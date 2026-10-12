//! Stage 15: bounded large-neighborhood search for geometric layouts and
//! conservative identical FULL-BOARD stack opportunities.
//!
//! Removes subsets of boards, reserves unaffected inventory, repairs the
//! outstanding pieces using the Stage 9 generators and sometimes clones a
//! repeated pattern before repair. Every candidate is revalidated by the
//! independent Stage 6 validator and repriced using Stage 10 stacking.
//! Strip stacking and physical machine scheduling are NOT inferred here.

use crate::full_board_stacking::{plan_full_board_stacks, FullBoardStackPlan, StackCostModel, StackPlanError};
use crate::layout_validation::{validate_layout, BoardAssignment, LayoutPlan};
use crate::model::{validate_model, CuttingJobModel, ModelIssue};
use crate::multi_board::{generate_multi_board, valid_multi_board_settings, AllocatedBoard, MultiBoardError, MultiBoardResult};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Instant;

fn default_iterations() -> usize { 8 }
fn default_neighborhood() -> usize { 3 }
fn default_operation_weight() -> u64 { 1 }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalSearchOptions {
    #[serde(default = "crate::multi_board::default_max_boards")]
    pub max_boards: usize,
    #[serde(default = "crate::multi_board::default_restarts")]
    pub restarts: usize,
    #[serde(default = "crate::three_stage::default_beam_width")]
    pub beam_width: usize,
    #[serde(default = "crate::three_stage::default_max_expansions")]
    pub max_expansions: usize,
    #[serde(default = "default_iterations")]
    pub max_iterations: usize,
    #[serde(default = "default_neighborhood")]
    pub neighborhood_boards: usize,
    /// Conversion of abstract Stage 10 work units to minor monetary units.
    /// This is a provisional proxy, NOT a calibrated industrial cost model.
    #[serde(default = "default_operation_weight")]
    pub operation_weight_minor: u64,
    #[serde(default)]
    pub stack_costs: StackCostModel,
}

impl Default for GlobalSearchOptions {
    fn default() -> Self {
        Self { max_boards: 128, restarts: 3, beam_width: 12,
            max_expansions: 1500, max_iterations: default_iterations(),
            neighborhood_boards: default_neighborhood(),
            operation_weight_minor: default_operation_weight(),
            stack_costs: StackCostModel::default() }
    }
}

pub fn valid_global_options(o: &GlobalSearchOptions) -> bool {
    valid_multi_board_settings(o.max_boards, o.restarts, o.beam_width, o.max_expansions)
        && (1..=64).contains(&o.max_iterations)
        && (1..=8).contains(&o.neighborhood_boards)
        && o.operation_weight_minor <= 1_000_000
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalError {
    InvalidSettings,
    InvalidModel(Vec<ModelIssue>),
    Allocation(MultiBoardError),
    Stacking(StackPlanError),
    InvalidCandidate(String),
}
impl fmt::Display for GlobalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSettings => write!(f, "invalid global search options"),
            Self::InvalidModel(issues) => write!(f, "invalid model ({} issues)", issues.len()),
            Self::Allocation(e) => write!(f, "multi-board allocator: {e}"),
            Self::Stacking(e) => write!(f, "stack estimator: {e}"),
            Self::InvalidCandidate(s) => write!(f, "invalid global candidate: {s}"),
        }
    }
}
impl std::error::Error for GlobalError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Improvement {
    pub iteration: usize,
    pub destroyed_boards: usize,
    pub used_pattern_repetition: bool,
    pub remaining_pieces: u64,
    pub boards: usize,
    pub full_board_stacks: usize,
    pub objective_minor: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalResult {
    pub initial: MultiBoardResult,
    pub initial_stacks: FullBoardStackPlan,
    pub best: MultiBoardResult,
    pub best_stacks: FullBoardStackPlan,
    pub initial_objective_minor: String,
    pub best_objective_minor: String,
    pub iterations_completed: usize,
    pub neighbors_validated: usize,
    pub improvements: Vec<Improvement>,
    pub deadline_reached: bool,
    pub note: String,
}

struct Candidate {
    layout: MultiBoardResult,
    stacks: FullBoardStackPlan,
    remaining: u64,
    objective: u128,
}

fn sum_remaining(r: &MultiBoardResult) -> u64 {
    r.remaining_part_counts.values().map(|&v| u64::from(v)).sum()
}

fn rank(model: &CuttingJobModel, result: MultiBoardResult, opts: GlobalSearchOptions)
    -> Result<Candidate, GlobalError> {
    let mut with_patterns = model.clone();
    with_patterns.patterns.extend(result.boards.iter().map(|b| b.pattern.clone()));
    let stacks = plan_full_board_stacks(&with_patterns, &result.layout, opts.stack_costs)
        .map_err(GlobalError::Stacking)?;
    let material = result.stock_cost_minor.parse::<u128>()
        .map_err(|e| GlobalError::InvalidCandidate(e.to_string()))?;
    let work = stacks.estimated_work_units_with_stacking.parse::<u128>()
        .map_err(|e| GlobalError::InvalidCandidate(e.to_string()))?;
    let objective = material.saturating_add(work.saturating_mul(u128::from(opts.operation_weight_minor)));
    Ok(Candidate { remaining: sum_remaining(&result), layout: result, stacks, objective })
}

fn is_better(new: &Candidate, old: &Candidate) -> bool {
    (new.remaining, new.objective, new.layout.boards.len(),
        u64::MAX - new.stacks.estimated_saw_passes_saved)
        < (old.remaining, old.objective, old.layout.boards.len(),
            u64::MAX - old.stacks.estimated_saw_passes_saved)
}

/// Rebuild stock-unit indices, names and aggregate bookkeeping after joining
/// preserved and newly-generated trees. Indices are interchangeable within
/// one stock SKU; they are never reused in a final layout.
fn merge_boards(model: &CuttingJobModel, mut boards: Vec<AllocatedBoard>)
    -> Result<MultiBoardResult, GlobalError> {
    let mut pending: BTreeMap<String, u32> = model.parts.iter()
        .map(|p| (p.id.clone(), p.quantity)).collect();
    let mut usage: BTreeMap<String, u32> = BTreeMap::new();
    let mut cost: u128 = 0;
    let mut area: u128 = 0;
    let stock_map: BTreeMap<&str, _> = model.stocks.iter()
        .map(|s| (s.id.as_str(), s)).collect();
    let reference_ids: BTreeSet<&str> = model.patterns.iter().map(|p| p.id.as_str()).collect();
    for (i, b) in boards.iter_mut().enumerate() {
        let stock = stock_map.get(b.stock_id.as_str()).ok_or_else(||
            GlobalError::InvalidCandidate(format!("unknown stock {}", b.stock_id)))?;
        let used = usage.entry(b.stock_id.clone()).or_default();
        if *used >= stock.quantity {
            return Err(GlobalError::InvalidCandidate("stock capacity exceeded".into()));
        }
        b.stock_unit_index = *used;
        *used += 1;
        b.board_id = format!("lns_board_{i:05}");
        let mut id = format!("lns_pattern_{i:05}");
        while reference_ids.contains(id.as_str()) { id.push('x'); }
        b.pattern.id = id.clone();
        b.evaluated.pattern_id = id;
        b.unit_cost_minor = stock.unit_cost_minor;
        cost = cost.saturating_add(u128::from(stock.unit_cost_minor));
        area = area.saturating_add(b.evaluated.summary.piece_area_sq_units.parse::<u128>()
            .map_err(|e| GlobalError::InvalidCandidate(e.to_string()))?);
        for (part_id, count) in &b.evaluated.summary.per_part_counts {
            let pending_qty = pending.get_mut(part_id).ok_or_else(||
                GlobalError::InvalidCandidate(format!("unknown part {part_id}")))?;
            *pending_qty = pending_qty.checked_sub(*count).ok_or_else(||
                GlobalError::InvalidCandidate(format!("part overproduced: {part_id}")))?;
        }
    }
    let mut with_patterns = model.clone();
    with_patterns.patterns.extend(boards.iter().map(|b| b.pattern.clone()));
    let layout = LayoutPlan { boards: boards.iter().map(|b| BoardAssignment {
        board_id: b.board_id.clone(), stock_id: b.stock_id.clone(),
        stock_unit_index: b.stock_unit_index, pattern_id: b.pattern.id.clone(),
        board_rotated_90: b.board_rotated_90,
    }).collect() };
    let validation = validate_layout(&with_patterns, &layout);
    let unexpected: Vec<String> = validation.issues.iter()
        .filter(|x| x.code != "missing_pieces" && x.code != "empty_layout")
        .map(|x| format!("{}:{}", x.path, x.code)).collect();
    if !unexpected.is_empty() || validation.summary.boards_geometry_verified != boards.len() {
        return Err(GlobalError::InvalidCandidate(format!("independent layout validation: {unexpected:?}")));
    }
    let complete_order = pending.values().all(|&v| v == 0) && validation.valid;
    Ok(MultiBoardResult {
        complete_order,
        search_exhausted: false,
        attempts_completed: 1,
        candidates_evaluated: 0,
        boards,
        layout,
        validation,
        remaining_part_counts: pending,
        stock_usage: usage,
        stock_cost_minor: cost.to_string(),
        placed_piece_area_sq_units: area.to_string(),
        note: "LNS reconstructed geometric candidate independently validated; physical cut scheduling NOT verified.".into(),
    })
}

fn remaining_model(model: &CuttingJobModel, locked: &[AllocatedBoard])
    -> Result<CuttingJobModel, GlobalError> {
    let mut reduced = model.clone();
    reduced.patterns.clear();
    reduced.workpieces.clear();
    reduced.stacks.clear();
    let mut used: BTreeMap<&str, u32> = BTreeMap::new();
    for b in locked {
        *used.entry(b.stock_id.as_str()).or_default() += 1;
        for (id, count) in &b.evaluated.summary.per_part_counts {
            let part = reduced.parts.iter_mut().find(|p| &p.id == id).ok_or_else(||
                GlobalError::InvalidCandidate(format!("unknown part {id}")))?;
            part.quantity = part.quantity.checked_sub(*count).ok_or_else(||
                GlobalError::InvalidCandidate("overproduced while retaining boards".into()))?;
        }
    }
    reduced.parts.retain(|p| p.quantity > 0);
    for s in &mut reduced.stocks {
        s.quantity = s.quantity.checked_sub(*used.get(s.id.as_str()).unwrap_or(&0))
            .ok_or_else(|| GlobalError::InvalidCandidate("stock exhausted by locked boards".into()))?;
    }
    reduced.stocks.retain(|s| s.quantity > 0);
    Ok(reduced)
}

fn repeat_first_available(reduced: &mut CuttingJobModel, donors: &[AllocatedBoard])
    -> Option<AllocatedBoard> {
    for donor in donors {
        let Some(stock_index) = reduced.stocks.iter().position(|s| s.id == donor.stock_id && s.quantity > 0) else { continue; };
        let can_replicate = donor.evaluated.summary.per_part_counts.iter().all(|(id, count)| {
            reduced.parts.iter().find(|p| &p.id == id)
                .is_some_and(|p| p.quantity >= *count)
        });
        if !can_replicate { continue; }
        for (id, count) in &donor.evaluated.summary.per_part_counts {
            if let Some(p) = reduced.parts.iter_mut().find(|p| &p.id == id) { p.quantity -= *count; }
        }
        reduced.parts.retain(|p| p.quantity > 0);
        reduced.stocks[stock_index].quantity -= 1;
        reduced.stocks.retain(|s| s.quantity > 0);
        return Some(donor.clone());
    }
    None
}

fn neighborhood_indices(boards: &[AllocatedBoard], iteration: usize, seed: u64, cap: usize)
    -> BTreeSet<usize> {
    let mut order: Vec<usize> = (0..boards.len()).collect();
    order.sort_by_key(|&i| (boards[i].utilization_basis_points,
        boards[i].board_id.clone()));
    let k = cap.min(boards.len()).max(1);
    if iteration % 4 == 3 { return (0..boards.len()).collect(); }
    let start = ((seed as usize).wrapping_add(iteration * 7)) % boards.len();
    // Every other iteration explicitly attacks low-utilization boards;
    // others explore rotating neighborhoods to escape a fixed greedy order.
    if iteration % 2 == 0 { order.into_iter().take(k).collect() }
    else { (0..k).map(|n| (start + n) % boards.len()).collect() }
}

fn repair(model: &CuttingJobModel, incumbent: &Candidate,
          opts: GlobalSearchOptions, seed: u64, iteration: usize, runtime_ms: u64)
    -> Result<Option<(Candidate, usize, bool)>, GlobalError> {
    let boards = &incumbent.layout.boards;
    if boards.is_empty() { return Ok(None); }
    let dropped = neighborhood_indices(boards, iteration, seed, opts.neighborhood_boards);
    let mut locked: Vec<AllocatedBoard> = boards.iter().enumerate()
        .filter(|(i, _)| !dropped.contains(i)).map(|(_, b)| b.clone()).collect();
    let mut reduced = remaining_model(model, &locked)?;
    let mut repeated = false;
    // Repeating preserved patterns is a targeted destroy/repair move that
    // raises the odds of economical full-board packages.
    if iteration % 2 == 1 {
        if let Some(copy) = repeat_first_available(&mut reduced, &locked) {
            locked.push(copy);
            repeated = true;
        }
    }
    let n_remaining: u64 = reduced.parts.iter().map(|p| u64::from(p.quantity)).sum();
    let mut new_boards = locked;
    if n_remaining > 0 {
        if reduced.stocks.is_empty() || new_boards.len() >= opts.max_boards { return Ok(None); }
        let generated = generate_multi_board(&reduced, seed.wrapping_add(iteration as u64 * 104729),
            runtime_ms, opts.max_boards - new_boards.len(), 1,
            opts.beam_width, opts.max_expansions).map_err(GlobalError::Allocation)?;
        new_boards.extend(generated.boards);
    }
    let result = merge_boards(model, new_boards)?;
    let candidate = rank(model, result, opts)?;
    Ok(Some((candidate, dropped.len(), repeated)))
}

pub fn optimize_global(model: &CuttingJobModel, seed: u64, runtime_ms: u64,
                       opts: GlobalSearchOptions) -> Result<GlobalResult, GlobalError> {
    validate_model(model).map_err(GlobalError::InvalidModel)?;
    if !valid_global_options(&opts) || runtime_ms < 100 || runtime_ms > 3_600_000 {
        return Err(GlobalError::InvalidSettings);
    }
    let start = Instant::now();
    // The initial allocator gets a guaranteed budget. The remaining budget is
    // shared by local-neighborhood repairs and independent validation.
    let baseline_ms = (runtime_ms / 2).max(100);
    let initial = generate_multi_board(model, seed, baseline_ms, opts.max_boards,
        opts.restarts, opts.beam_width, opts.max_expansions)
        .map_err(GlobalError::Allocation)?;
    let initial = rank(model, initial, opts)?;
    let initial_result = initial.layout.clone();
    let initial_stacks = initial.stacks.clone();
    let initial_objective = initial.objective.to_string();
    let mut best = initial;
    let mut validated = 0_usize;
    let mut iterations = 0_usize;
    let mut improvements = Vec::new();
    for iteration in 0..opts.max_iterations {
        let used = start.elapsed().as_millis() as u64;
        if used >= runtime_ms || runtime_ms.saturating_sub(used) < 110 { break; }
        let left = runtime_ms - used;
        let remaining_iterations = (opts.max_iterations - iteration) as u64;
        let slice = (left / remaining_iterations).max(100).min(left);
        // Some neighbors will be rejected (over-budget / impossible repair)
        // and this does not invalidate the last independently checked best.
        match repair(model, &best, opts, seed, iteration, slice) {
            Ok(Some((candidate, destroyed, repeated))) => {
                validated += 1;
                if is_better(&candidate, &best) {
                    improvements.push(Improvement {
                        iteration: iteration + 1,
                        destroyed_boards: destroyed,
                        used_pattern_repetition: repeated,
                        remaining_pieces: candidate.remaining,
                        boards: candidate.layout.boards.len(),
                        full_board_stacks: candidate.stacks.stacks.len(),
                        objective_minor: candidate.objective.to_string(),
                    });
                    best = candidate;
                }
            }
            Ok(None) | Err(GlobalError::InvalidCandidate(_)) => {},
            Err(err) => return Err(err),
        }
        iterations += 1;
    }
    let deadline_reached = start.elapsed().as_millis() as u64 >= runtime_ms;
    Ok(GlobalResult {
        initial: initial_result,
        initial_stacks,
        best: best.layout,
        best_stacks: best.stacks,
        initial_objective_minor: initial_objective,
        best_objective_minor: best.objective.to_string(),
        iterations_completed: iterations,
        neighbors_validated: validated,
        improvements,
        deadline_reached,
        note: "Bounded geometric LNS plus cost proxy for IDENTICAL FULL-BOARD patterns; not a globally optimal solution. Strip apilados, partial common cuts, real machine time, and a runnable physical production sequence are NOT jointly optimized in this stage.".into(),
    })
}

