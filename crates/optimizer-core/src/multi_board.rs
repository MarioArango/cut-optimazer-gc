//! Stage 9: bounded multi-board allocation over Stage 7/8 pattern generators.
//!
//! Builds several boards from currently outstanding parts, reserves concrete
//! stock-unit indices, attempts alternative sheet-selection policies, and
//! cross-checks the resulting batch with the independent Stage 6 validator.
//! This is a heuristic allocator, not a proof of infeasibility, an exact
//! global optimum, or a physical machine/stack schedule.

use crate::guillotine::{EvaluatedPattern, TreeError};
use crate::layout_validation::{validate_layout, BoardAssignment, LayoutPlan, LayoutValidationReport};
use crate::model::{validate_model, CuttingJobModel, CuttingPattern, ModelIssue, StagePattern};
use crate::deep_stage::{generate_deep_stage_weighted, DeepStageError, UNIT_VALUE};
use crate::three_stage::{generate_three_stage, valid_search_settings, ThreeStageError};
use crate::two_stage::{generate_two_stage, GeneratorError};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

const MAX_BOARD_LIMIT: usize = 512;
const MAX_RESTARTS: usize = 8;
/// Value-corrected passes per requested restart (4+ level patterns only).
const VALUE_PASSES_PER_RESTART: usize = 2;
const MAX_STOCK_VARIANTS: usize = 128;

pub fn default_max_boards() -> usize { 128 }
pub fn default_restarts() -> usize { 3 }

pub fn valid_multi_board_settings(max_boards: usize, restarts: usize,
                                  beam_width: usize, max_expansions: usize) -> bool {
    (1..=MAX_BOARD_LIMIT).contains(&max_boards)
        && (1..=MAX_RESTARTS).contains(&restarts)
        && valid_search_settings(beam_width, max_expansions)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultiBoardError {
    InvalidModel(Vec<ModelIssue>),
    InvalidSettings,
    TooManyStockTypes(usize),
    TwoStage(GeneratorError),
    ThreeStage(ThreeStageError),
    DeepStage(DeepStageError),
    InvalidGeneratedPattern(TreeError),
    InternalValidationFailed(Vec<String>),
}
impl fmt::Display for MultiBoardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModel(issues) => write!(f, "invalid model ({} issue(s))", issues.len()),
            Self::InvalidSettings => write!(f, "max_boards must be 1..=512, restarts 1..=8; beam_width and max_expansions must meet the stage 8 bounds"),
            Self::TooManyStockTypes(n) => write!(f, "multi-board search supports at most {MAX_STOCK_VARIANTS} stock types (received {n})"),
            Self::TwoStage(e) => write!(f, "two-stage search: {e}"),
            Self::ThreeStage(e) => write!(f, "three-stage search: {e}"),
            Self::DeepStage(e) => write!(f, "multi-level search: {e}"),
            Self::InvalidGeneratedPattern(e) => write!(f, "invalid generated pattern: {e}"),
            Self::InternalValidationFailed(codes) => write!(f, "generated layout failed independent validation: {}", codes.join(", ")),
        }
    }
}
impl std::error::Error for MultiBoardError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternSource {
    TwoStage,
    ThreeStage,
    /// 4..=6 cutting levels (`deep_stage`).
    DeepStage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocatedBoard {
    pub board_id: String,
    pub stock_id: String,
    pub stock_unit_index: u32,
    pub mode: StagePattern,
    pub source: PatternSource,
    pub board_rotated_90: bool,
    pub utilization_basis_points: u32,
    pub unit_cost_minor: u64,
    pub pattern: CuttingPattern,
    pub evaluated: EvaluatedPattern,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultiBoardResult {
    pub complete_order: bool,
    /// True if a deadline or the board limit interrupted a search. It is not
    /// a proof that remaining pieces can or cannot be manufactured.
    pub search_exhausted: bool,
    pub attempts_completed: usize,
    pub candidates_evaluated: usize,
    pub boards: Vec<AllocatedBoard>,
    pub layout: LayoutPlan,
    pub validation: LayoutValidationReport,
    pub remaining_part_counts: BTreeMap<String, u32>,
    pub stock_usage: BTreeMap<String, u32>,
    /// Monetary units match Stock.unit_cost_minor; u128 emitted as text for JS.
    pub stock_cost_minor: String,
    /// Squared tenths-of-a-millimeter, exact numeric text for JS clients.
    pub placed_piece_area_sq_units: String,
    pub note: String,
}

#[derive(Clone)]
struct BoardOption {
    source: PatternSource,
    stock_index: usize,
    board_rotated_90: bool,
    mode: StagePattern,
    utilization_basis_points: u32,
    pattern: CuttingPattern,
    evaluated: EvaluatedPattern,
    placed_count: u64,
    placed_area: u128,
    /// Placed area weighted by part values (value-corrected passes only).
    placed_value: u128,
    stock_cost: u64,
    usable_area: u128,
}

#[derive(Clone)]
struct Attempt {
    boards: Vec<AllocatedBoard>,
    remaining: BTreeMap<String, u32>,
    stock_usage: BTreeMap<String, u32>,
    material_cost: u128,
    placed_area: u128,
    candidates: usize,
    interrupted: bool,
}

fn remaining_total(remaining: &BTreeMap<String, u32>) -> u64 {
    remaining.values().map(|&n| u64::from(n)).sum()
}

fn score_order(a: &BoardOption, b: &BoardOption, policy: usize) -> Ordering {
    // Prices can be 0 (not entered yet). Treat these as 1 for ranking only;
    // real reported material cost always uses the actual inventory price.
    let ac = u128::from(a.stock_cost.max(1));
    let bc = u128::from(b.stock_cost.max(1));
    match policy % 4 {
        // More fitted piece area per unit cost; avoid an arbitrarily expensive
        // stock type when dimensions are equivalent.
        0 => (a.placed_area * bc).cmp(&(b.placed_area * ac))
            .then(a.placed_area.cmp(&b.placed_area)),
        1 => a.placed_area.cmp(&b.placed_area)
            .then((a.placed_area * bc).cmp(&(b.placed_area * ac))),
        2 => a.placed_count.cmp(&b.placed_count)
            .then(a.placed_area.cmp(&b.placed_area)),
        // Prefer compact patterns when both are feasible, useful when the
        // inventory has disparate sizes and a small order remains.
        _ => (a.placed_area * b.usable_area).cmp(&(b.placed_area * a.usable_area))
            .then((a.placed_area * bc).cmp(&(b.placed_area * ac))),
    }
    .then(a.placed_count.cmp(&b.placed_count))
    .then(b.stock_cost.cmp(&a.stock_cost))
    .then(a.utilization_basis_points.cmp(&b.utilization_basis_points))
    // Determinism for equal scores. Prefer inventory order, then the
    // shorter, simpler two-stage plan when all other metrics tie.
    .then(b.stock_index.cmp(&a.stock_index))
    .then(match (a.source, b.source) {
        (PatternSource::TwoStage, PatternSource::ThreeStage) => Ordering::Greater,
        (PatternSource::ThreeStage, PatternSource::TwoStage) => Ordering::Less,
        _ => Ordering::Equal,
    })
}

fn better_attempt(a: &Attempt, b: &Attempt) -> bool {
    let ap = remaining_total(&a.remaining);
    let bp = remaining_total(&b.remaining);
    // Completion and fulfilled unit count are mandatory before economy.
    // Among complete orders: first cost, then number of boards, then area.
    if ap != bp { return ap < bp; }
    if a.material_cost != b.material_cost { return a.material_cost < b.material_cost; }
    if a.boards.len() != b.boards.len() { return a.boards.len() < b.boards.len(); }
    if a.placed_area != b.placed_area { return a.placed_area > b.placed_area; }
    false
}

fn pending_model(model: &CuttingJobModel, counts: &BTreeMap<String, u32>) -> CuttingJobModel {
    let mut reduced = model.clone();
    reduced.parts.retain_mut(|part| {
        part.quantity = counts.get(&part.id).copied().unwrap_or(0);
        part.quantity != 0
    });
    // The supplied reference patterns and workpieces may legitimately point
    // to part types now completely fulfilled. These aren't inputs to the
    // pattern generator; remove them ONLY from this temporary search model.
    reduced.patterns.clear();
    reduced.workpieces.clear();
    reduced.stacks.clear();
    reduced
}

fn candidate_from(
    source: PatternSource, stock_index: usize, model: &CuttingJobModel,
    pattern: CuttingPattern, evaluated: EvaluatedPattern,
    rotated: bool, mode: StagePattern, utilization_basis_points: u32,
) -> BoardOption {
    let stock = &model.stocks[stock_index];
    let placed_count = evaluated.summary.per_part_counts.values().map(|&q| u64::from(q)).sum();
    let placed_area = evaluated.summary.piece_area_sq_units.parse::<u128>().unwrap_or(0);
    let usable_area = evaluated.summary.usable_area_sq_units.parse::<u128>().unwrap_or(1).max(1);
    BoardOption {
        source, stock_index, board_rotated_90: rotated, mode,
        utilization_basis_points, pattern, evaluated, placed_count,
        placed_area, placed_value: placed_area * u128::from(UNIT_VALUE), stock_cost: stock.unit_cost_minor, usable_area,
    }
}

#[allow(clippy::too_many_arguments)]
fn generate_options(
    model: &CuttingJobModel, pending: &CuttingJobModel,
    reserved: &[u32], seed: u64, beam_width: usize, max_expansions: usize,
    deadline: Instant, max_runtime_ms: u64, weights: Option<&BTreeMap<String, u64>>,
) -> Result<(Vec<BoardOption>, usize), MultiBoardError> {
    let mut options = Vec::new();
    let mut evaluated_count = 0;
    let uses_two = model.machine.allowed_patterns.iter().any(|mode| matches!(mode, StagePattern::VH | StagePattern::HV));
    let uses_three = model.machine.allowed_patterns.iter().any(|mode| matches!(mode, StagePattern::VHV | StagePattern::HVH));
    // Deeper patterns get their own generator; with 2-3 levels it never runs,
    // so those results are exactly what they were.
    let uses_deep = model.machine.allowed_patterns.iter().any(|mode| mode.levels() >= 4);
    for (index, stock) in model.stocks.iter().enumerate() {
        if reserved[index] >= stock.quantity { continue; }
        if deadline.elapsed().as_millis() as u64 >= max_runtime_ms { break; }
        // Per-call time cap avoids spending the entire job budget exploring
        // one expensive stock. An individual generator may still stop early.
        let left = max_runtime_ms.saturating_sub(deadline.elapsed().as_millis() as u64);
        if left < 100 { break; }
        let slice = left.min(400).max(100);
        if uses_two {
            let r = generate_two_stage(pending, Some(&stock.id), seed, slice).map_err(MultiBoardError::TwoStage)?;
            evaluated_count += r.candidates_evaluated;
            if let Some(board) = r.board {
                options.push(candidate_from(PatternSource::TwoStage, index, model, board.pattern,
                    board.evaluated, board.board_rotated_90, board.mode, board.utilization_basis_points));
            }
        }
        if uses_three && (deadline.elapsed().as_millis() as u64) < max_runtime_ms {
            let left = max_runtime_ms.saturating_sub(deadline.elapsed().as_millis() as u64);
            if left < 100 { break; }
            let slice = left.min(400).max(100);
            let r = generate_three_stage(pending, Some(&stock.id), seed, slice,
                beam_width, max_expansions).map_err(MultiBoardError::ThreeStage)?;
            evaluated_count += r.candidates_evaluated;
            if let Some(board) = r.board {
                options.push(candidate_from(PatternSource::ThreeStage, index, model, board.pattern,
                    board.evaluated, board.board_rotated_90, board.mode, board.utilization_basis_points));
            }
        }
        if uses_deep && (deadline.elapsed().as_millis() as u64) < max_runtime_ms {
            let left = max_runtime_ms.saturating_sub(deadline.elapsed().as_millis() as u64);
            if left < 100 { break; }
            let slice = left.min(400).max(100);
            let r = generate_deep_stage_weighted(pending, Some(&stock.id), seed, slice,
                beam_width, max_expansions, weights).map_err(MultiBoardError::DeepStage)?;
            evaluated_count += r.candidates_evaluated;
            if let Some(board) = r.board {
                options.push(candidate_from(PatternSource::DeepStage, index, model, board.pattern,
                    board.evaluated, board.board_rotated_90, board.mode, board.utilization_basis_points));
            }
        }
    }
    if let Some(weights) = weights {
        for option in &mut options {
            option.placed_value = weighted_value(model, &option.evaluated, weights);
        }
    }
    Ok((options, evaluated_count))
}

/// Piece area of a board weighted by part values.
fn weighted_value(model: &CuttingJobModel, evaluated: &EvaluatedPattern, weights: &BTreeMap<String, u64>) -> u128 {
    evaluated.summary.per_part_counts.iter().map(|(id, &count)| {
        let area = model.parts.iter().find(|p| &p.id == id)
            .map_or(0, |p| u128::from(p.width) * u128::from(p.height));
        u128::from(count) * area * u128::from(weights.get(id).copied().unwrap_or(UNIT_VALUE))
    }).sum()
}

/// Sequential value correction: a part placed on poorly used boards becomes
/// more valuable, so the next pass places it earlier, in better company.
/// The new value blends the old one with the waste factor of its boards
/// (usable area / placed area, in thousandths); unplaced parts double.
fn correct_values(model: &CuttingJobModel, attempt: &Attempt, weights: &mut BTreeMap<String, u64>) {
    let mut seen: BTreeMap<&str, (u128, u128)> = BTreeMap::new();
    for board in &attempt.boards {
        let usable: u128 = board.evaluated.summary.usable_area_sq_units.parse().unwrap_or(0);
        let placed: u128 = board.evaluated.summary.piece_area_sq_units.parse().unwrap_or(0);
        if placed == 0 { continue; }
        let factor = usable * u128::from(UNIT_VALUE) / placed;
        for piece in &board.evaluated.pieces {
            let area = u128::from(piece.rect.width) * u128::from(piece.rect.height);
            let entry = seen.entry(piece.part_id.as_str()).or_insert((0, 0));
            entry.0 += factor * area;
            entry.1 += area;
        }
    }
    for part in &model.parts {
        let old = weights.get(&part.id).copied().unwrap_or(UNIT_VALUE);
        let target = match seen.get(part.id.as_str()) {
            Some(&(weighted, area)) if area > 0 => (weighted / area) as u64,
            _ => old.saturating_mul(2),
        };
        let unplaced = attempt.remaining.get(&part.id).copied().unwrap_or(0) > 0;
        let target = if unplaced { target.max(old.saturating_mul(2)) } else { target };
        weights.insert(part.id.clone(), ((old + target) / 2).clamp(1, 1_000_000));
    }
}

#[allow(clippy::too_many_arguments)]
fn make_attempt(
    model: &CuttingJobModel, seed: u64, policy: usize, max_boards: usize,
    beam_width: usize, max_expansions: usize, started: Instant, max_runtime_ms: u64,
    weights: Option<&BTreeMap<String, u64>>,
) -> Result<Attempt, MultiBoardError> {
    let mut remaining: BTreeMap<String, u32> = model.parts.iter()
        .map(|part| (part.id.clone(), part.quantity)).collect();
    let mut reserved = vec![0_u32; model.stocks.len()];
    let mut boards = Vec::new();
    let mut total_cost = 0_u128;
    let mut total_area = 0_u128;
    let mut candidates = 0_usize;
    let mut interrupted = false;
    while remaining_total(&remaining) > 0 && boards.len() < max_boards {
        if started.elapsed().as_millis() as u64 >= max_runtime_ms {
            interrupted = true;
            break;
        }
        let search = pending_model(model, &remaining);
        let (options, count) = generate_options(model, &search, &reserved,
            seed.wrapping_add(boards.len() as u64 * 37), beam_width,
            max_expansions, started, max_runtime_ms, weights)?;
        candidates += count;
        let selected = options.into_iter().filter(|c| c.placed_count > 0)
            .max_by(|a, b| match weights {
                // Value-corrected pass: most valuable board first.
                Some(_) => a.placed_value.cmp(&b.placed_value).then_with(|| score_order(a, b, policy)),
                None => score_order(a, b, policy),
            });
        let Some(option) = selected else {
            if started.elapsed().as_millis() as u64 >= max_runtime_ms { interrupted = true; }
            break;
        };
        let stock = &model.stocks[option.stock_index];
        let stock_unit_index = reserved[option.stock_index];
        reserved[option.stock_index] += 1;
        let serial = boards.len() + 1;
        let mut pattern = option.pattern;
        let mut evaluated = option.evaluated;
        pattern.id = format!("s9_pattern_{serial:05}");
        while model.patterns.iter().any(|p| p.id == pattern.id) {
            pattern.id.push('x');
        }
        evaluated.pattern_id = pattern.id.clone();
        let placed = &evaluated.summary.per_part_counts;
        for (part_id, &count) in placed {
            let pending_count = remaining.get_mut(part_id).ok_or_else(||
                MultiBoardError::InternalValidationFailed(vec![format!("unknown generated part {part_id}")]))?;
            *pending_count = pending_count.checked_sub(count).ok_or_else(||
                MultiBoardError::InternalValidationFailed(vec![format!("overproduced part {part_id}")]))?;
        }
        total_cost += u128::from(option.stock_cost);
        total_area += option.placed_area;
        boards.push(AllocatedBoard {
            board_id: format!("s9_board_{serial:05}"),
            stock_id: stock.id.clone(), stock_unit_index,
            mode: option.mode, source: option.source,
            board_rotated_90: option.board_rotated_90,
            utilization_basis_points: option.utilization_basis_points,
            unit_cost_minor: option.stock_cost,
            pattern, evaluated,
        });
    }
    if remaining_total(&remaining) > 0 && boards.len() >= max_boards { interrupted = true; }
    Ok(Attempt {
        boards,
        remaining,
        stock_usage: model.stocks.iter().zip(reserved)
            .filter(|(_, count)| *count > 0)
            .map(|(stock, count)| (stock.id.clone(), count)).collect(),
        material_cost: total_cost,
        placed_area: total_area,
        candidates,
        interrupted,
    })
}

/// Heuristic multi-board packing. Each board is independently evaluated and
/// the final batch is independently checked for exact quantities and stock
/// ownership. A partial order can be returned safely and explicitly marked.
pub fn generate_multi_board(
    model: &CuttingJobModel, seed: u64, max_runtime_ms: u64,
    max_boards: usize, restarts: usize, beam_width: usize, max_expansions: usize,
) -> Result<MultiBoardResult, MultiBoardError> {
    validate_model(model).map_err(MultiBoardError::InvalidModel)?;
    if !valid_multi_board_settings(max_boards, restarts, beam_width, max_expansions) {
        return Err(MultiBoardError::InvalidSettings);
    }
    if model.stocks.len() > MAX_STOCK_VARIANTS {
        return Err(MultiBoardError::TooManyStockTypes(model.stocks.len()));
    }
    let started = Instant::now();
    let mut best: Option<Attempt> = None;
    let mut total_candidates = 0_usize;
    let mut attempts = 0_usize;
    let mut exhausted = false;
    // With 4+ cut levels the boards come out dense enough that greedy
    // board-by-board filling strands the hard pieces on nearly empty boards.
    // Value-corrected passes fix that; with 2-3 levels nothing changes.
    let value_corrected = model.machine.allowed_patterns.iter().any(|m| m.levels() >= 4);
    let passes = if value_corrected { restarts * VALUE_PASSES_PER_RESTART } else { restarts };
    let mut weights: BTreeMap<String, u64> = model.parts.iter().map(|p| (p.id.clone(), UNIT_VALUE)).collect();
    for pass in 0..passes {
        if (started.elapsed().as_millis() as u64) >= max_runtime_ms {
            exhausted = true;
            break;
        }
        let attempt = make_attempt(model, seed.wrapping_add(pass as u64 * 1_000_003),
            pass, max_boards, beam_width, max_expansions, started, max_runtime_ms,
            value_corrected.then_some(&weights))?;
        attempts += 1;
        total_candidates += attempt.candidates;
        exhausted |= attempt.interrupted;
        if value_corrected { correct_values(model, &attempt, &mut weights); }
        if best.as_ref().is_none_or(|old| better_attempt(&attempt, old)) {
            best = Some(attempt);
        }
    }
    let winner = best.unwrap_or_else(|| Attempt {
        boards: vec![],
        remaining: model.parts.iter().map(|p| (p.id.clone(), p.quantity)).collect(),
        stock_usage: BTreeMap::new(), material_cost: 0, placed_area: 0,
        candidates: 0, interrupted: true,
    });
    let mut validation_model = model.clone();
    validation_model.patterns.extend(winner.boards.iter().map(|b| b.pattern.clone()));
    let layout = LayoutPlan { boards: winner.boards.iter().map(|b| BoardAssignment {
        board_id: b.board_id.clone(),
        stock_id: b.stock_id.clone(),
        stock_unit_index: b.stock_unit_index,
        pattern_id: b.pattern.id.clone(),
        board_rotated_90: b.board_rotated_90,
    }).collect() };
    let validation = validate_layout(&validation_model, &layout);
    // For a partial order the independent validator reports missing_pieces
    // (or empty_layout). Any other failure means the generator is incorrect.
    let unexpected: Vec<String> = validation.issues.iter()
        .filter(|issue| issue.code != "missing_pieces" && issue.code != "empty_layout")
        .map(|issue| format!("{}:{}", issue.path, issue.code)).collect();
    if !unexpected.is_empty() || validation.summary.boards_geometry_verified != winner.boards.len() {
        return Err(MultiBoardError::InternalValidationFailed(unexpected));
    }
    let complete = remaining_total(&winner.remaining) == 0 && validation.valid;
    let note = if complete {
        "Complete multi-board geometric layout validated. Machine execution and stacking have NOT been scheduled or validated."
    } else {
        "Partial geometric layout: some pieces remain unallocated. Search exhaustion does not prove infeasibility; machine execution and stacking are NOT validated."
    };
    Ok(MultiBoardResult {
        complete_order: complete, search_exhausted: exhausted,
        attempts_completed: attempts, candidates_evaluated: total_candidates,
        boards: winner.boards, layout, validation,
        remaining_part_counts: winner.remaining,
        stock_usage: winner.stock_usage,
        stock_cost_minor: winner.material_cost.to_string(),
        placed_piece_area_sq_units: winner.placed_area.to_string(),
        note: note.into(),
    })
}
