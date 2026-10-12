//! Stage 12: deterministic, physically verified common-cut strip stacking.
//!
//! Given an explicit Stage 11 production prefix and one *next cut* per strip,
//! finds compatible free strips across origins, or retains an already formed
//! strip stack, and produces executable STACK + CUT_STACK (or CUT_STACK alone)
//! operations. The entire augmented trace is verified by Stage 11's independent
//! simulator. Dynamic re-stacking and choosing when to unstack are Stage 13.

use crate::geometry::Rect;
use crate::model::{CutDirection, CuttingJobModel, Orientation, RegistrationEdge, Unit, WorkpieceKind};
use crate::production_simulator::{simulate_production, PhysicalState, PhysicalWorkpiece,
    ProductionOperation, ProductionPlan, ProductionSimulation, SimulationError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const MAX_INTENTS: usize = 10_000;
const MAX_GROUPS: usize = 500;

fn default_max_groups() -> usize { 100 }
fn default_cut_cost() -> u64 { 150 }
fn default_stack_cost() -> u64 { 35 }
fn default_exit_cost() -> u64 { 25 }
fn default_true() -> bool { true }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StripStackOptions {
    #[serde(default = "default_max_groups")]
    pub max_groups: usize,
    /// Estimated cost per saw pass, in minor currency units.
    #[serde(default = "default_cut_cost")]
    pub saw_pass_cost_minor: u64,
    /// Cost for creating one new stack (not charged for a maintained stack).
    #[serde(default = "default_stack_cost")]
    pub stack_handling_cost_minor: u64,
    /// Reserve for subsequent separation/handling of the output packages.
    #[serde(default = "default_exit_cost")]
    pub followup_handling_cost_minor: u64,
    #[serde(default = "default_true")]
    pub require_positive_savings: bool,
    #[serde(default = "default_true")]
    pub include_existing_stacks: bool,
}
impl Default for StripStackOptions {
    fn default() -> Self {
        Self { max_groups: default_max_groups(), saw_pass_cost_minor: default_cut_cost(),
            stack_handling_cost_minor: default_stack_cost(),
            followup_handling_cost_minor: default_exit_cost(),
            require_positive_savings: true, include_existing_stacks: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextStripCut {
    pub workpiece_id: String,
    pub direction: CutDirection,
    /// Beginning of saw channel, in 0.1mm from current registered edge.
    pub at: Unit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupOrigin { NewlyStacked, AlreadyStacked }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StripStackGroup {
    pub group_id: String,
    pub origin: GroupOrigin,
    pub member_ids: Vec<String>,
    pub source_board_ids: Vec<String>,
    pub input_stack_id: String,
    pub first_output_stack_id: String,
    pub second_output_stack_id: String,
    pub direction: CutDirection,
    pub at: Unit,
    pub total_height: u64,
    pub individual_saw_passes: u64,
    pub shared_saw_passes: u64,
    pub estimated_savings_minor: i128,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StripStackResult {
    pub groups: Vec<StripStackGroup>,
    pub operations_added: Vec<ProductionOperation>,
    pub estimated_net_savings_minor: String,
    pub saw_passes_avoided: u64,
    pub ungrouped_workpiece_ids: Vec<String>,
    pub base_simulation: ProductionSimulation,
    pub projected_simulation: ProductionSimulation,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StripStackError {
    Simulation(SimulationError),
    InvalidInput { code: String, detail: String },
}
impl fmt::Display for StripStackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Simulation(err) => write!(f, "strip plan simulation: {err}"),
            Self::InvalidInput { code, detail } => write!(f, "strip planning [{code}]: {detail}"),
        }
    }
}
impl std::error::Error for StripStackError {}
fn invalid(code: &str, detail: impl Into<String>) -> StripStackError {
    StripStackError::InvalidInput { code: code.into(), detail: detail.into() }
}

// All registration-critical values plus NEXT operation must agree.
// BTree ordering makes the grouping and serialized result reproducible.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Signature {
    material_id: String,
    width: Unit,
    height: Unit,
    thickness: Unit,
    grain: u8,
    orientation: u8,
    registration: u8,
    station: String,
    cut_direction: u8,
    at: Unit,
}
fn grain_code(axis: Option<crate::model::Axis>) -> u8 {
    match axis { None => 0, Some(crate::model::Axis::X) => 1, Some(crate::model::Axis::Y) => 2 }
}
fn direction_code(d: CutDirection) -> u8 {
    match d { CutDirection::Vertical => 0, CutDirection::Horizontal => 1 }
}
fn signature(p: &PhysicalWorkpiece, next: &NextStripCut) -> Signature {
    Signature { material_id: p.material_id.clone(), width: p.width, height: p.height,
        thickness: p.thickness, grain: grain_code(p.grain_axis),
        orientation: match p.orientation { Orientation::Deg0 => 0, Orientation::Deg90 => 1 },
        registration: match p.registration_edge { RegistrationEdge::Left => 0,
            RegistrationEdge::Right => 1, RegistrationEdge::Front => 2, RegistrationEdge::Back => 3 },
        station: p.station.clone(), cut_direction: direction_code(next.direction), at: next.at }
}
fn is_strip(p: &PhysicalWorkpiece) -> bool {
    matches!(p.kind, WorkpieceKind::Strip | WorkpieceKind::Region)
}
fn candidate_savings(size: usize, already: bool, opts: StripStackOptions) -> i128 {
    (size.saturating_sub(1) as i128) * i128::from(opts.saw_pass_cost_minor)
        - if already { 0 } else { i128::from(opts.stack_handling_cost_minor) }
        - i128::from(opts.followup_handling_cost_minor)
}
fn generate_ids(sequence: usize, used: &mut BTreeSet<String>) -> [String; 3] {
    let mut suffix = sequence;
    loop {
        let group = format!("s12_{suffix:05}");
        let ids = [format!("{group}_in"), format!("{group}_a"), format!("{group}_b")];
        if ids.iter().all(|id| !used.contains(id)) {
            for id in &ids { used.insert(id.to_string()); }
            return ids;
        }
        suffix += 1;
    }
}
fn generated_child_ids(source_ids: &[String], side: &str, used: &mut BTreeSet<String>) -> Vec<String> {
    source_ids.iter().map(|id| {
        let mut candidate = format!("s12_{side}_{id}");
        let mut suffix = 0usize;
        while used.contains(&candidate) || candidate.len() > 80 {
            candidate = format!("s12_{side}_{suffix:08}");
            suffix += 1;
        }
        used.insert(candidate.clone());
        candidate
    }).collect()
}

/// Stage 12: propose and execute only the *next common cut* for each selected
/// strip package. Does not choose later separation/recombination (Stage 13).
/// Returns a projected execution, not machine certification.
pub fn plan_strip_stacks(
    model: &CuttingJobModel,
    prefix: &ProductionPlan,
    next_cuts: &[NextStripCut],
    options: StripStackOptions,
) -> Result<StripStackResult, StripStackError> {
    if !(1..=MAX_GROUPS).contains(&options.max_groups) || next_cuts.len() > MAX_INTENTS {
        return Err(invalid("limits", "max_groups must be 1..500 and next_cuts <=10000"));
    }
    let base = simulate_production(model, prefix).map_err(StripStackError::Simulation)?;
    let pieces: BTreeMap<&str, &PhysicalWorkpiece> = base.workpieces.iter().map(|p| (p.id.as_str(), p)).collect();
    let mut intents = BTreeMap::<String, &NextStripCut>::new();
    for cut in next_cuts {
        if intents.insert(cut.workpiece_id.clone(), cut).is_some() {
            return Err(invalid("duplicate_intent", &cut.workpiece_id));
        }
        let p = pieces.get(cut.workpiece_id.as_str())
            .ok_or_else(|| invalid("unknown_workpiece", &cut.workpiece_id))?;
        if p.state != PhysicalState::Active || !is_strip(p) {
            return Err(invalid("ineligible_workpiece", format!("{} is not an active strip/region", cut.workpiece_id)));
        }
        let path_len = match cut.direction { CutDirection::Vertical => p.height, CutDirection::Horizontal => p.width };
        if path_len > model.machine.max_cut_length {
            return Err(invalid("cut_length", &cut.workpiece_id));
        }
        let split = Rect::new(0, 0, p.width, p.height)
            .and_then(|r| r.split(cut.direction, cut.at, model.machine.kerf))
            .map_err(|e| invalid("invalid_next_cut", format!("{}: {e}", cut.workpiece_id)))?;
        if split.first.width < model.machine.min_piece_width || split.first.height < model.machine.min_piece_height
            || split.second.width < model.machine.min_piece_width || split.second.height < model.machine.min_piece_height {
            return Err(invalid("minimum_dimensions", &cut.workpiece_id));
        }
    }

    let mut candidates: Vec<(bool, Option<String>, Vec<String>, i128)> = Vec::new();
    if options.include_existing_stacks && model.machine.stacking.strips {
        for stack in &base.stacks {
            let members: Vec<&PhysicalWorkpiece> = stack.member_ids.iter().filter_map(|id| pieces.get(id.as_str()).copied()).collect();
            if members.len() != stack.member_ids.len() || !members.iter().all(|p| is_strip(p)) { continue; }
            if members.len() < 2 || !members.iter().all(|p| intents.contains_key(&p.id)) { continue; }
            let first_sig = signature(members[0], intents[&members[0].id]);
            if members.iter().all(|p| signature(p, intents[&p.id]) == first_sig) {
                let score = candidate_savings(members.len(), true, options);
                if score > 0 || !options.require_positive_savings {
                    candidates.push((true, Some(stack.id.clone()), stack.member_ids.clone(), score));
                }
            }
        }
    }
    if model.machine.stacking.strips && model.machine.stacking.restacking {
        let mut buckets = BTreeMap::<Signature, Vec<&PhysicalWorkpiece>>::new();
        for p in pieces.values() {
            if p.state == PhysicalState::Active && p.stack_id.is_none() && is_strip(p) {
                if let Some(intent) = intents.get(&p.id) {
                    buckets.entry(signature(p, intent)).or_default().push(p);
                }
            }
        }
        for (_key, mut bucket) in buckets {
            bucket.sort_by(|a, b| a.id.cmp(&b.id));
            let thickness = u64::from(bucket[0].thickness);
            let height_limit = u64::from(model.machine.max_stack_height) / thickness;
            let limit = usize::try_from(height_limit.min(u64::from(model.machine.max_stack_members)))
                .unwrap_or(usize::MAX);
            if limit < 2 { continue; }
            // Deterministically choose large groups. Never leave a singleton that
            // could have joined another viable group if a 2+2 partition exists.
            let mut offset = 0;
            while bucket.len() - offset >= 2 {
                let left = bucket.len() - offset;
                let count = if left > limit && left - limit == 1 { limit - 1 } else { left.min(limit) };
                if count < 2 { break; }
                let member_ids: Vec<String> = bucket[offset..offset + count].iter().map(|p| p.id.clone()).collect();
                let score = candidate_savings(count, false, options);
                if score > 0 || !options.require_positive_savings {
                    candidates.push((false, None, member_ids, score));
                }
                offset += count;
            }
        }
    }
    // Higher estimated savings first, tie-broken deterministically by source ID.
    candidates.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.2.cmp(&b.2)));
    let mut used = BTreeSet::<String>::new();
    for p in &base.workpieces { used.insert(p.id.clone()); }
    for s in &base.stacks { used.insert(s.id.clone()); }
    for event in &base.events {
        match &event.operation {
            ProductionOperation::Stack { stack_id, .. } | ProductionOperation::Unstack { stack_id }
            | ProductionOperation::RotateStack { stack_id } | ProductionOperation::MoveStack { stack_id, .. } => {
                used.insert(stack_id.clone());
            }
            ProductionOperation::CutStack { stack_id, first_stack_id, second_stack_id, .. } => {
                used.insert(stack_id.clone());
                used.insert(first_stack_id.clone());
                used.insert(second_stack_id.clone());
            }
            _ => {}
        }
    }
    let mut groups = Vec::new();
    let mut operations = Vec::new();
    let mut accepted = BTreeSet::new();
    let mut net_savings: i128 = 0;
    let mut avoided: u64 = 0;
    for (already, existing_id, members, score) in candidates {
        if groups.len() >= options.max_groups || members.iter().any(|id| accepted.contains(id)) { continue; }
        let ids = generate_ids(groups.len() + 1, &mut used);
        let input_stack_id = existing_id.unwrap_or_else(|| ids[0].clone());
        let direction = intents[&members[0]].direction;
        let at = intents[&members[0]].at;
        let first_ids = generated_child_ids(&members, "a", &mut used);
        let second_ids = generated_child_ids(&members, "b", &mut used);
        let mut trial = operations.clone();
        if !already {
            trial.push(ProductionOperation::Stack { stack_id: input_stack_id.clone(), member_ids: members.clone() });
        }
        trial.push(ProductionOperation::CutStack {
            stack_id: input_stack_id.clone(), direction, at,
            first_stack_id: ids[1].clone(), second_stack_id: ids[2].clone(),
            first_member_ids: first_ids, second_member_ids: second_ids,
        });
        let mut trial_plan = prefix.clone();
        trial_plan.operations.extend(trial.iter().cloned());
        // Replaying the exact trace validates simultaneous cutting and all
        // package/lineage/material invariants before the recommendation is kept.
        if simulate_production(model, &trial_plan).is_err() { continue; }
        operations = trial;
        for id in &members { accepted.insert(id.clone()); }
        let source_boards = members.iter().map(|id| pieces[id.as_str()].origin_board_id.clone()).collect();
        let total_height: u64 = members.iter().map(|id| u64::from(pieces[id.as_str()].thickness)).sum();
        net_savings += score;
        avoided += members.len() as u64 - 1;
        groups.push(StripStackGroup { group_id: ids[0].clone(),
            origin: if already { GroupOrigin::AlreadyStacked } else { GroupOrigin::NewlyStacked },
            member_ids: members.clone(), source_board_ids: source_boards,
            input_stack_id, first_output_stack_id: ids[1].clone(), second_output_stack_id: ids[2].clone(),
            direction, at, total_height, individual_saw_passes: members.len() as u64,
            shared_saw_passes: 1, estimated_savings_minor: score,
        });
    }
    let mut full_plan = prefix.clone();
    full_plan.operations.extend(operations.iter().cloned());
    let projected = simulate_production(model, &full_plan).map_err(StripStackError::Simulation)?;
    let ungrouped = intents.keys().filter(|id| !accepted.contains(*id)).cloned().collect();
    // An empty group list is a valid result (e.g., machine forbids strip stacking).
    Ok(StripStackResult { groups, operations_added: operations,
        estimated_net_savings_minor: net_savings.to_string(), saw_passes_avoided: avoided,
        ungrouped_workpiece_ids: ungrouped, base_simulation: base, projected_simulation: projected,
        note: "Stage 12 shares the NEXT physical cut on compatible strips/regions. It respects material, grain, registered edge, orientation, station, stack limits and lineage via complete simulator replay. Savings are heuristic. No automatic re-stacking or optimal unstack timing (Stage 13); no machine-program certification (Stage 14).".into() })
}
