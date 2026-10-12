//! Stage 14 — constraint-aware ordering of supplied physical operations.
//!
//! The input is an unordered collection of explicit operations (not a CNC
//! program). Search explores feasible topological orders, checking *every*
//! candidate through the Stage 11 physical simulator and enforcing per-lineage
//! guillotine direction stages. Machine-specific clamps/feed paths and physical
//! setup times remain approximation/require a model-specific adapter.
use crate::model::{CutDirection, CuttingJobModel, Orientation, StagePattern};
use crate::production_simulator::{simulate_production, PhysicalState, ProductionOperation,
    ProductionPlan, ProductionSimulation, SimulationError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, Instant};

fn default_max_nodes() -> usize { 12_000 }
fn default_saw_cost() -> u64 { 100 }
fn default_stack_cost() -> u64 { 40 }
fn default_unstack_cost() -> u64 { 30 }
fn default_rotate_cost() -> u64 { 15 }
fn default_move_cost() -> u64 { 20 }
fn default_clamp_check() -> bool { true }

/// `before` and `after` refer to zero-based indices in the unordered input
/// operations. All dependencies are mandatory, in addition to implicit physical
/// dependencies enforced by replaying the simulator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleDependency { pub before: usize, pub after: usize }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleOptions {
    #[serde(default = "default_max_nodes")]
    pub max_search_nodes: usize,
    #[serde(default)]
    pub require_terminal: bool,
    /// Simple conservative pre-check, not a vendor-specific clamp simulation.
    #[serde(default = "default_clamp_check")]
    pub require_min_clamp_footprint: bool,
    #[serde(default = "default_saw_cost")]
    pub saw_pass_cost_minor: u64,
    #[serde(default = "default_stack_cost")]
    pub stack_cost_minor: u64,
    #[serde(default = "default_unstack_cost")]
    pub unstack_cost_minor: u64,
    #[serde(default = "default_rotate_cost")]
    pub rotate_cost_minor: u64,
    #[serde(default = "default_move_cost")]
    pub move_cost_minor: u64,
}
impl Default for ScheduleOptions {
    fn default() -> Self {
        Self { max_search_nodes: default_max_nodes(), require_terminal: false,
            require_min_clamp_footprint: true, saw_pass_cost_minor: default_saw_cost(),
            stack_cost_minor: default_stack_cost(), unstack_cost_minor: default_unstack_cost(),
            rotate_cost_minor: default_rotate_cost(), move_cost_minor: default_move_cost() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledStep {
    pub order: usize,
    pub original_index: usize,
    pub action: String,
    pub saw_passes_after: u64,
    pub active_stacks_after: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleResult {
    pub ordered_source_indices: Vec<usize>,
    pub scheduled_plan: ProductionPlan,
    pub steps: Vec<ScheduledStep>,
    pub simulation: ProductionSimulation,
    pub search_nodes: usize,
    pub estimated_operation_cost_minor: String,
    pub cut_passes: u64,
    pub stack_operations: usize,
    pub unstack_operations: usize,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchedulerError {
    InvalidInput { code: String, message: String },
    NoFeasibleOrder { scheduled: usize, reason: String },
    SearchBudgetExceeded { explored: usize },
    Simulation(SimulationError),
}
impl fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput { code, message } => write!(f, "schedule [{code}]: {message}"),
            Self::NoFeasibleOrder { scheduled, reason } =>
                write!(f, "no feasible production order after {scheduled} operations: {reason}"),
            Self::SearchBudgetExceeded { explored } => write!(f, "schedule search budget exceeded after {explored} candidate replays"),
            Self::Simulation(err) => write!(f, "schedule simulation: {err}"),
        }
    }
}
impl std::error::Error for SchedulerError {}
fn invalid(code: &str, message: impl Into<String>) -> SchedulerError {
    SchedulerError::InvalidInput { code: code.into(), message: message.into() }
}

fn cut_axis(direction: CutDirection) -> u8 {
    match direction { CutDirection::Vertical => 0, CutDirection::Horizontal => 1 }
}
fn pattern_axes(pattern: StagePattern) -> &'static [u8] {
    match pattern { StagePattern::VH => &[0, 1], StagePattern::HV => &[1, 0],
        StagePattern::VHV => &[0, 1, 0], StagePattern::HVH => &[1, 0, 1] }
}

#[derive(Clone)]
struct SearchState {
    used: Vec<bool>,
    order: Vec<usize>,
    history: BTreeMap<String, Vec<u8>>,
    simulation: ProductionSimulation,
}

fn initial_rotation(plan: &ProductionPlan) -> BTreeMap<String, bool> {
    plan.initial_boards.iter().map(|b| (b.board_id.clone(), b.rotated_90)).collect()
}

/// Stage directions are tracked in the original stock frame, so rotating an
/// intermediate piece changes the physical saw direction but not its history.
fn canonical_axis(direction: CutDirection, orientation: Orientation, origin_rotated: bool) -> u8 {
    let flipped = (orientation == Orientation::Deg90) != origin_rotated;
    cut_axis(direction) ^ u8::from(flipped)
}

fn check_stages(model: &CuttingJobModel, history: &[u8], next: u8) -> bool {
    let mut stages = Vec::new();
    for axis in history.iter().copied().chain(std::iter::once(next)) {
        if stages.last() != Some(&axis) { stages.push(axis); }
    }
    model.machine.allowed_patterns.iter().any(|pattern| pattern_axes(*pattern).starts_with(&stages))
}

fn update_histories(
    model: &CuttingJobModel, state: &SearchState, op: &ProductionOperation,
    original_rotation: &BTreeMap<String, bool>, clamp_check: bool,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut histories = state.history.clone();
    let lookup = |id: &str| state.simulation.workpieces.iter().find(|p| p.id == id && p.state == PhysicalState::Active)
        .ok_or_else(|| format!("active workpiece '{id}' is not available"));
    let mut advance = |parent: &str, children: &[&String], direction: CutDirection| -> Result<(), String> {
        let piece = lookup(parent)?;
        if clamp_check && (piece.width < model.machine.min_clamp_width || piece.height < model.machine.min_clamp_width) {
            return Err(format!("clamp footprint too small for '{parent}'"));
        }
        let axis = canonical_axis(direction, piece.orientation,
            *original_rotation.get(&piece.origin_board_id).unwrap_or(&false));
        let hist = histories.get(parent).cloned().unwrap_or_default();
        if !check_stages(model, &hist, axis) {
            return Err(format!("guillotine stage sequence would be invalid on '{parent}'"));
        }
        let mut new_hist = hist;
        new_hist.push(axis);
        histories.remove(parent);
        for id in children { histories.insert((**id).clone(), new_hist.clone()); }
        Ok(())
    };
    match op {
        ProductionOperation::CutWorkpiece { workpiece_id, first_id, second_id, direction, .. } =>
            advance(workpiece_id, &[first_id, second_id], *direction)?,
        ProductionOperation::CutStack { stack_id, first_member_ids, second_member_ids, direction, .. } => {
            let stack = state.simulation.stacks.iter().find(|s| s.id == *stack_id)
                .ok_or_else(|| format!("stack '{stack_id}' is not available"))?;
            if stack.member_ids.len() != first_member_ids.len() || stack.member_ids.len() != second_member_ids.len() {
                return Err(format!("wrong number of output layers for '{stack_id}'"));
            }
            for (i, parent) in stack.member_ids.iter().enumerate() {
                advance(parent, &[&first_member_ids[i], &second_member_ids[i]], *direction)?;
            }
        }
        _ => {}
    }
    Ok(histories)
}

fn op_action(op: &ProductionOperation) -> &'static str {
    match op {
        ProductionOperation::Stack { .. } => "stack", ProductionOperation::Unstack { .. } => "unstack",
        ProductionOperation::CutWorkpiece { .. } => "cut_workpiece", ProductionOperation::CutStack { .. } => "cut_stack",
        ProductionOperation::RotateWorkpiece { .. } => "rotate_workpiece", ProductionOperation::RotateStack { .. } => "rotate_stack",
        ProductionOperation::MoveWorkpiece { .. } => "move_workpiece", ProductionOperation::MoveStack { .. } => "move_stack",
        ProductionOperation::Finish { .. } => "finish", ProductionOperation::Scrap { .. } => "scrap",
    }
}
fn order_weight(op: &ProductionOperation) -> i32 {
    match op {
        ProductionOperation::CutStack { .. } => 0,
        ProductionOperation::Stack { .. } => 1,
        ProductionOperation::CutWorkpiece { .. } => 2,
        ProductionOperation::Unstack { .. } => 3,
        ProductionOperation::Finish { .. } | ProductionOperation::Scrap { .. } => 4,
        ProductionOperation::RotateStack { .. } | ProductionOperation::RotateWorkpiece { .. } => 5,
        ProductionOperation::MoveStack { .. } | ProductionOperation::MoveWorkpiece { .. } => 6,
    }
}
struct Scheduler<'a> {
    model: &'a CuttingJobModel,
    input: &'a ProductionPlan,
    deps: &'a [ScheduleDependency],
    options: ScheduleOptions,
    start: Instant,
    max_runtime: Duration,
    nodes: usize,
    max_depth: usize,
    last_blocker: String,
    origins: BTreeMap<String, bool>,
}
impl<'a> Scheduler<'a> {
    fn dfs(&mut self, state: SearchState) -> Result<Option<SearchState>, SchedulerError> {
        if state.order.len() == self.input.operations.len() {
            if !self.options.require_terminal ||
                (state.simulation.summary.live_workpieces == 0 && state.simulation.summary.active_stacks == 0) {
                return Ok(Some(state));
            }
            self.last_blocker = "terminal plan still has active material or packages".into();
            return Ok(None);
        }
        self.max_depth = self.max_depth.max(state.order.len());
        let mut candidates: Vec<usize> = (0..self.input.operations.len()).filter(|&i| {
            !state.used[i] && self.deps.iter().filter(|d| d.after == i).all(|d| state.used[d.before])
        }).collect();
        candidates.sort_by_key(|&i| (order_weight(&self.input.operations[i]), i));
        let mut local_reason = String::new();
        for i in candidates {
            if self.nodes >= self.options.max_search_nodes || self.start.elapsed() > self.max_runtime {
                return Err(SchedulerError::SearchBudgetExceeded { explored: self.nodes });
            }
            self.nodes += 1;
            let op = &self.input.operations[i];
            let history = match update_histories(self.model, &state, op,
                &self.origins, self.options.require_min_clamp_footprint) {
                Ok(value) => value,
                Err(err) => { if local_reason.is_empty() { local_reason = err; } continue; },
            };
            let mut ordered_ops = Vec::with_capacity(state.order.len() + 1);
            for &index in &state.order { ordered_ops.push(self.input.operations[index].clone()); }
            ordered_ops.push(op.clone());
            let trial_plan = ProductionPlan { initial_boards: self.input.initial_boards.clone(), operations: ordered_ops };
            let simulation = match simulate_production(self.model, &trial_plan) {
                Ok(s) => s,
                Err(err) => { if local_reason.is_empty() { local_reason = err.to_string(); } continue; },
            };
            let mut used = state.used.clone();
            used[i] = true;
            let mut order = state.order.clone();
            order.push(i);
            let next = SearchState { used, order, history, simulation };
            if let Some(success) = self.dfs(next)? { return Ok(Some(success)); }
        }
        if !local_reason.is_empty() { self.last_blocker = local_reason; }
        if self.last_blocker.is_empty() {
            self.last_blocker = "dependency cycle or no available operations".to_owned();
        }
        Ok(None)
    }
}

/// Validate scheduler configuration, references and initial stock without
/// searching for a feasible order. Used by request parsing to avoid running
/// the full bounded search multiple times per CLI invocation.
pub fn validate_schedule_input(
    model: &CuttingJobModel, unordered: &ProductionPlan,
    dependencies: &[ScheduleDependency], options: ScheduleOptions, max_runtime_ms: u64,
) -> Result<(), SchedulerError> {
    let n = unordered.operations.len();
    if n > 256 || options.max_search_nodes == 0 || options.max_search_nodes > 100_000 ||
        !(100..=3_600_000).contains(&max_runtime_ms) {
        return Err(invalid("limits", "maximum 256 operations, 1..100000 nodes and 100..3600000 ms"));
    }
    let mut dep_set = BTreeSet::new();
    for d in dependencies {
        if d.before >= n || d.after >= n || d.before == d.after {
            return Err(invalid("dependency", "dependency references invalid or identical operation indices"));
        }
        if !dep_set.insert((d.before, d.after)) {
            return Err(invalid("duplicate_dependency", "the same dependency is repeated"));
        }
    }
    let empty = ProductionPlan { initial_boards: unordered.initial_boards.clone(), operations: vec![] };
    simulate_production(model, &empty).map_err(SchedulerError::Simulation)?;
    Ok(())
}

/// Order operations without changing their geometry or their identities.
/// Guarantees a replayable physical trace and pattern-prefix-valid cut stages,
/// but does not certify a particular machine controller or exact clamp reach.
pub fn schedule_production(
    model: &CuttingJobModel, unordered: &ProductionPlan,
    dependencies: &[ScheduleDependency], options: ScheduleOptions, max_runtime_ms: u64,
) -> Result<ScheduleResult, SchedulerError> {
    validate_schedule_input(model, unordered, dependencies, options, max_runtime_ms)?;
    let n = unordered.operations.len();
    let empty = ProductionPlan { initial_boards: unordered.initial_boards.clone(), operations: vec![] };
    let baseline = simulate_production(model, &empty).map_err(SchedulerError::Simulation)?;
    let mut scheduler = Scheduler { model, input: unordered, deps: dependencies, options,
        start: Instant::now(), max_runtime: Duration::from_millis(max_runtime_ms), nodes: 0,
        max_depth: 0, last_blocker: String::new(), origins: initial_rotation(unordered) };
    let seed = SearchState { used: vec![false; n], order: vec![], history: BTreeMap::new(), simulation: baseline };
    let done = scheduler.dfs(seed)?.ok_or_else(|| SchedulerError::NoFeasibleOrder {
        scheduled: scheduler.max_depth, reason: scheduler.last_blocker.clone(),
    })?;
    let ordered_ops: Vec<_> = done.order.iter().map(|&i| unordered.operations[i].clone()).collect();
    let scheduled_plan = ProductionPlan { initial_boards: unordered.initial_boards.clone(), operations: ordered_ops };
    let steps = done.simulation.events.iter().zip(done.order.iter()).enumerate()
        .map(|(i, (event, source))| Ok(ScheduledStep { order: i + 1, original_index: *source,
            action: event.action.clone(), saw_passes_after: done.simulation.events[..=i].iter()
                .map(|e| u64::from(e.saw_passes)).sum(),
            active_stacks_after: simulate_production(model, &ProductionPlan {
                initial_boards: unordered.initial_boards.clone(),
                operations: scheduled_plan.operations[..=i].to_vec(),
            }).map_err(SchedulerError::Simulation)?.summary.active_stacks,
        })).collect::<Result<Vec<_>, SchedulerError>>()?;
    let mut stack_count = 0;
    let mut unstack_count = 0;
    let mut cost: u128 = u128::from(done.simulation.summary.saw_passes) * u128::from(options.saw_pass_cost_minor);
    for op in &scheduled_plan.operations {
        let increment = match op {
            ProductionOperation::Stack { .. } => { stack_count += 1; options.stack_cost_minor },
            ProductionOperation::Unstack { .. } => { unstack_count += 1; options.unstack_cost_minor },
            ProductionOperation::RotateWorkpiece { .. } | ProductionOperation::RotateStack { .. } => options.rotate_cost_minor,
            ProductionOperation::MoveWorkpiece { .. } | ProductionOperation::MoveStack { .. } => options.move_cost_minor,
            _ => 0,
        };
        cost += u128::from(increment);
    }
    Ok(ScheduleResult { ordered_source_indices: done.order,
        cut_passes: done.simulation.summary.saw_passes, stack_operations: stack_count,
        unstack_operations: unstack_count, search_nodes: scheduler.nodes,
        estimated_operation_cost_minor: cost.to_string(), steps,
        scheduled_plan, simulation: done.simulation,
        note: "Replay-validated order with lineage-specific V-H/H-V stage constraints. This is an operation plan, NOT certified CNC code; physical clamp reach, saw setup, trimming strategy and handling-time calibration require a machine-specific adapter.".into(),
    })
}
