//! Stage 13 — deterministic, bounded dynamic stacking/unstacking of cut regions.
//!
//! This is a *physical-operation planner*, not a machine-code postprocessor.
//! It follows a binary cut route for each active strip/region, shares compatible
//! cuts between layers, releases packages when their routes diverge, and may
//! regroup previously cut strips from different source boards. Each committed
//! operation is replayed by the independent Stage 11 production simulator.
//! Future machine clamp constraints and factory scheduling belong to Stage 14.

use crate::geometry::Rect;
use crate::model::{CutDirection, CuttingJobModel, WorkpieceKind};
use crate::production_simulator::{simulate_production, PhysicalState, PhysicalWorkpiece,
    ProductionOperation, ProductionPlan, ProductionSimulation, SimulationError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const MAX_ROUTES: usize = 5_000;
const MAX_ROUTE_NODES: usize = 20_000;
const MAX_OPERATIONS: usize = 20_000;

fn default_max_operations() -> usize { 2_000 }
fn default_saw_cost() -> u64 { 150 }
fn default_stack_cost() -> u64 { 35 }
fn default_unstack_cost() -> u64 { 25 }
fn default_true() -> bool { true }

/// A cut route is an explicit binary tree, **not** simply a list of cuts.
/// Both output regions receive their own continuation. `done` marks a terminal
/// physical region, without implicitly turning it into a finished saleable part.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CutRoute {
    Done,
    Cut {
        direction: CutDirection,
        at: u32,
        first: Box<CutRoute>,
        second: Box<CutRoute>,
    },
}

impl CutRoute {
    fn next_cut(&self) -> Option<(CutDirection, u32)> {
        match self {
            Self::Done => None,
            Self::Cut { direction, at, .. } => Some((*direction, *at)),
        }
    }
    fn count_cuts(&self) -> usize {
        match self {
            Self::Done => 0,
            Self::Cut { first, second, .. } => 1 + first.count_cuts() + second.count_cuts(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkpieceRoute {
    pub workpiece_id: String,
    pub route: CutRoute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicStackOptions {
    #[serde(default = "default_max_operations")]
    pub max_operations: usize,
    #[serde(default = "default_saw_cost")]
    pub saw_pass_cost_minor: u64,
    #[serde(default = "default_stack_cost")]
    pub stack_cost_minor: u64,
    #[serde(default = "default_unstack_cost")]
    pub unstack_cost_minor: u64,
    /// Release output stacks after all supplied routes finish.
    #[serde(default = "default_true")]
    pub release_finished_stacks: bool,
    /// Allow breaking an already-valid package to include a free, compatible
    /// strip if the incremental shared saw pass justifies the handling costs.
    #[serde(default = "default_true")]
    pub merge_with_existing: bool,
}
impl Default for DynamicStackOptions {
    fn default() -> Self {
        Self { max_operations: default_max_operations(), saw_pass_cost_minor: default_saw_cost(),
            stack_cost_minor: default_stack_cost(), unstack_cost_minor: default_unstack_cost(),
            release_finished_stacks: true, merge_with_existing: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind { KeepAndCut, StackAndCut, CutAlone, UnstackDivergent,
    UnstackForRegroup, ReleaseFinished }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackDecision {
    pub kind: DecisionKind,
    pub member_ids: Vec<String>,
    pub operation_from: usize,
    pub operation_count: usize,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicStackResult {
    pub decisions: Vec<StackDecision>,
    pub operations_added: Vec<ProductionOperation>,
    pub base_simulation: ProductionSimulation,
    pub projected_simulation: ProductionSimulation,
    pub standalone_saw_passes: u64,
    pub planned_saw_passes: u64,
    pub saw_passes_avoided: u64,
    pub estimated_net_savings_minor: String,
    pub terminal_workpiece_ids: Vec<String>,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DynamicStackError {
    Simulation(SimulationError),
    InvalidInput { code: String, message: String },
}
impl fmt::Display for DynamicStackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Simulation(e) => write!(f, "dynamic stack simulation: {e}"),
            Self::InvalidInput { code, message } => write!(f, "dynamic stacks [{code}]: {message}"),
        }
    }
}
impl std::error::Error for DynamicStackError {}
fn invalid(code: &str, message: impl Into<String>) -> DynamicStackError {
    DynamicStackError::InvalidInput { code: code.into(), message: message.into() }
}

/// Strictly registration-compatible signature. Cut intention is compared
/// separately because packages can have diverging future routes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Signature {
    material: String, width: u32, height: u32, thickness: u32,
    grain: u8, orientation: u8, registration: u8, station: String,
    direction: u8, at: u32,
}
fn signature(p: &PhysicalWorkpiece, cut: (CutDirection, u32)) -> Signature {
    use crate::model::{Axis, Orientation, RegistrationEdge};
    Signature {
        material: p.material_id.clone(), width: p.width, height: p.height,
        thickness: p.thickness,
        grain: match p.grain_axis { None => 0, Some(Axis::X) => 1, Some(Axis::Y) => 2 },
        orientation: match p.orientation { Orientation::Deg0 => 0, Orientation::Deg90 => 1 },
        registration: match p.registration_edge { RegistrationEdge::Left => 0,
            RegistrationEdge::Right => 1, RegistrationEdge::Front => 2, RegistrationEdge::Back => 3 },
        station: p.station.clone(),
        direction: match cut.0 { CutDirection::Vertical => 0, CutDirection::Horizontal => 1 },
        at: cut.1,
    }
}

struct Planner<'a> {
    model: &'a CuttingJobModel,
    prefix: ProductionPlan,
    options: DynamicStackOptions,
    routes: BTreeMap<String, CutRoute>,
    sim: ProductionSimulation,
    initial_sim: ProductionSimulation,
    additions: Vec<ProductionOperation>,
    decisions: Vec<StackDecision>,
    used_ids: BTreeSet<String>,
    next_id: usize,
}

impl<'a> Planner<'a> {
    fn fresh(&mut self) -> String {
        loop {
            self.next_id += 1;
            let candidate = format!("s13_{:07}", self.next_id);
            if self.used_ids.insert(candidate.clone()) { return candidate; }
        }
    }
    fn current_plan(&self, extra: &[ProductionOperation]) -> ProductionPlan {
        let mut plan = self.prefix.clone();
        plan.operations.extend(self.additions.iter().cloned());
        plan.operations.extend(extra.iter().cloned());
        plan
    }
    /// Only committed transformations alter the active route map.
    /// Reject a candidate before mutating the planner when simulator replay fails.
    fn commit(&mut self, ops: Vec<ProductionOperation>, kind: DecisionKind,
        members: Vec<String>, explanation: &str) -> Result<(), DynamicStackError> {
        if self.prefix.operations.len() + self.additions.len() + ops.len() > MAX_OPERATIONS
            || self.additions.len() + ops.len() > self.options.max_operations {
            return Err(invalid("operation_limit", "too many production operations for this request"));
        }
        let projected = simulate_production(self.model, &self.current_plan(&ops))
            .map_err(DynamicStackError::Simulation)?;
        let mut updated_routes = self.routes.clone();
        for op in &ops {
            match op {
                ProductionOperation::CutWorkpiece { workpiece_id, first_id, second_id, .. } => {
                    Self::advance_route(&mut updated_routes, workpiece_id, first_id, second_id)?;
                }
                ProductionOperation::CutStack { stack_id: source_id, first_member_ids, second_member_ids, .. } => {
                    // Every CUT_STACK is committed after any STACK action.
                    // Source layer order is read from the current simulator.
                    let stack_members = self.sim.stacks.iter().find(|s| &s.id == source_id)
                        .ok_or_else(|| invalid("missing_stack", source_id))?.member_ids.clone();
                    if stack_members.len() != first_member_ids.len() || stack_members.len() != second_member_ids.len() {
                        return Err(invalid("cut_layers", "child layers do not match input stack"));
                    }
                    for (index, source) in stack_members.iter().enumerate() {
                        Self::advance_route(&mut updated_routes, source,
                            &first_member_ids[index], &second_member_ids[index])?;
                    }
                }
                _ => {}
            }
        }
        let before = self.additions.len();
        self.additions.extend(ops);
        self.routes = updated_routes;
        self.sim = projected;
        self.decisions.push(StackDecision { kind, member_ids: members,
            operation_from: before, operation_count: self.additions.len() - before,
            explanation: explanation.to_owned() });
        Ok(())
    }
    fn advance_route(map: &mut BTreeMap<String, CutRoute>, source: &str,
        first_id: &str, second_id: &str) -> Result<(), DynamicStackError> {
        let route = map.remove(source).ok_or_else(|| invalid("missing_route", source))?;
        if let CutRoute::Cut { first, second, .. } = route {
            map.insert(first_id.to_owned(), *first);
            map.insert(second_id.to_owned(), *second);
            Ok(())
        } else {
            Err(invalid("completed_route", format!("{source} has no pending cut")))
        }
    }
    fn pending(&self, id: &str) -> Option<(CutDirection, u32)> {
        self.routes.get(id).and_then(CutRoute::next_cut)
    }
    fn can_assemble_strips(&self) -> bool {
        self.model.machine.stacking.strips && self.model.machine.stacking.restacking
    }
    fn sim_piece(&self, id: &str) -> Option<&PhysicalWorkpiece> {
        self.sim.workpieces.iter().find(|p| p.id == id)
    }
    fn stack_members_can_cut(&self, members: &[String]) -> Option<(CutDirection, u32)> {
        let first = members.first().and_then(|id| self.pending(id))?;
        let first_piece = self.sim_piece(&members[0])?;
        let sig = signature(first_piece, first);
        if members.iter().all(|id| self.pending(id) == Some(first)
            && self.sim_piece(id).is_some_and(|p| signature(p, first) == sig)) {
            Some(first)
        } else { None }
    }
    fn free_groups(&self) -> BTreeMap<Signature, Vec<String>> {
        let mut groups = BTreeMap::<Signature, Vec<String>>::new();
        for p in &self.sim.workpieces {
            if p.state == PhysicalState::Active && p.stack_id.is_none()
                && matches!(p.kind, WorkpieceKind::Strip | WorkpieceKind::Region) {
                if let Some(next) = self.pending(&p.id) {
                    groups.entry(signature(p, next)).or_default().push(p.id.clone());
                }
            }
        }
        groups
    }
    fn max_members(&self, thickness: u32) -> usize {
        if thickness == 0 { return 0; }
        usize::try_from(u64::from(self.model.machine.max_stack_height) / u64::from(thickness))
            .unwrap_or(usize::MAX)
            .min(usize::try_from(self.model.machine.max_stack_members).unwrap_or(usize::MAX))
    }
    /// Additional industrial minimum-dimension screen. The Stage 11 simulator
    /// independently checks geometry and stock conservation after each action.
    fn check_cut_dimensions(&self, id: &str, next: (CutDirection, u32))
        -> Result<(), DynamicStackError> {
        let piece = self.sim_piece(id).ok_or_else(|| invalid("unknown_workpiece", id))?;
        let split = Rect::new(0, 0, piece.width, piece.height)
            .and_then(|rect| rect.split(next.0, next.1, self.model.machine.kerf))
            .map_err(|e| invalid("invalid_cut_geometry", format!("{id}: {e}")))?;
        let min_w = self.model.machine.min_piece_width;
        let min_h = self.model.machine.min_piece_height;
        if split.first.width < min_w || split.first.height < min_h
            || split.second.width < min_w || split.second.height < min_h {
            return Err(invalid("minimum_dimensions", id));
        }
        Ok(())
    }
    fn cut_stack(&mut self, stack_id: &str, member_ids: &[String],
        next: (CutDirection, u32), kind: DecisionKind) -> Result<(), DynamicStackError> {
        for id in member_ids { self.check_cut_dimensions(id, next)?; }
        let first_stack_id = self.fresh();
        let second_stack_id = self.fresh();
        let first_member_ids: Vec<String> = member_ids.iter().map(|_| self.fresh()).collect();
        let second_member_ids: Vec<String> = member_ids.iter().map(|_| self.fresh()).collect();
        self.commit(vec![ProductionOperation::CutStack { stack_id: stack_id.to_owned(),
                direction: next.0, at: next.1, first_stack_id, second_stack_id,
                first_member_ids, second_member_ids }], kind, member_ids.to_vec(),
            "Corte compartido; ambas regiones de salida conservan sus capas y rutas")
    }
    fn cut_single(&mut self, id: &str, next: (CutDirection, u32)) -> Result<(), DynamicStackError> {
        self.check_cut_dimensions(id, next)?;
        let first_id = self.fresh();
        let second_id = self.fresh();
        self.commit(vec![ProductionOperation::CutWorkpiece { workpiece_id: id.into(),
            direction: next.0, at: next.1, first_id, second_id }], DecisionKind::CutAlone,
            vec![id.to_owned()], "Corte individual; el paquete no aportaría ahorro neto")
    }
    fn run(&mut self) -> Result<(), DynamicStackError> {
        // Each iteration must advance a route, increase the size of a ready
        // compatible package, or free a divergent package. Bounded execution.
        loop {
            if !self.routes.values().any(|r| r.next_cut().is_some()) { break; }
            // Existing packages: if their next cuts disagree, or some member
            // is already terminal, they must be released before further work.
            if let Some(stack) = self.sim.stacks.iter().find(|s|
                s.member_ids.iter().any(|m| self.pending(m).is_some())
                && self.stack_members_can_cut(&s.member_ids).is_none()).cloned() {
                self.commit(vec![ProductionOperation::Unstack { stack_id: stack.id }],
                    DecisionKind::UnstackDivergent, stack.member_ids,
                    "Rutas divergentes: separar el paquete antes del siguiente corte")?;
                continue;
            }
            // Optional release and re-stacking of an existing package with a
            // free strip to gain an additional saved saw pass.
            if self.can_assemble_strips() && self.options.merge_with_existing {
                let groups = self.free_groups();
                let mut repackage: Option<(String, Vec<String>)> = None;
                for stack in &self.sim.stacks {
                    if stack.member_ids.iter().all(|id| self.sim_piece(id).is_some_and(|p|
                        matches!(p.kind, WorkpieceKind::Strip | WorkpieceKind::Region))) {
                        if let Some(next) = self.stack_members_can_cut(&stack.member_ids) {
                            let p = self.sim_piece(&stack.member_ids[0]).expect("checked");
                            let max = self.max_members(p.thickness);
                            if max <= stack.member_ids.len() { continue; }
                            if let Some(free) = groups.get(&signature(p, next)) {
                                let extra = (max - stack.member_ids.len()).min(free.len());
                                if extra == 0 { continue; }
                                let gross = (extra as u128) * u128::from(self.options.saw_pass_cost_minor);
                                let handling = u128::from(self.options.unstack_cost_minor)
                                    + u128::from(self.options.stack_cost_minor);
                                if gross > handling {
                                    let mut members = stack.member_ids.clone();
                                    members.extend_from_slice(&free[..extra]);
                                    repackage = Some((stack.id.clone(), members));
                                    break;
                                }
                            }
                        }
                    }
                }
                if let Some((old_stack, members)) = repackage {
                    let new_id = self.fresh();
                    self.commit(vec![ProductionOperation::Unstack { stack_id: old_stack },
                        ProductionOperation::Stack { stack_id: new_id, member_ids: members.clone() }],
                        DecisionKind::UnstackForRegroup, members,
                        "Desapilar y reagrupar con franjas libres: una pasada adicional ahorrada cubre la manipulación")?;
                    continue;
                }
            }
            // Execute ready existing packages before creating new ones.
            if let Some((id, members, next)) = self.sim.stacks.iter().find_map(|s|
                self.stack_members_can_cut(&s.member_ids)
                    .map(|next| (s.id.clone(), s.member_ids.clone(), next))) {
                self.cut_stack(&id, &members, next, DecisionKind::KeepAndCut)?;
                continue;
            }
            let groups = self.free_groups();
            if self.can_assemble_strips() {
                // Among equally compatible next cuts, group equal *future*
                // routes first, to reduce subsequent forced separation.
                let mut best_group: Option<Vec<String>> = None;
                for (_key, ids) in &groups {
                    let first = self.sim_piece(&ids[0]).expect("present");
                    let limit = self.max_members(first.thickness);
                    if limit < 2 || ids.len() < 2 { continue; }
                    let mut ordered = ids.clone();
                    ordered.sort_by_key(|id| (format!("{:?}", self.routes.get(id)), id.clone()));
                    let count = limit.min(ordered.len());
                    let net = (count.saturating_sub(1) as u128) * u128::from(self.options.saw_pass_cost_minor);
                    let handling = u128::from(self.options.stack_cost_minor)
                        + u128::from(self.options.unstack_cost_minor);
                    if net <= handling { continue; }
                    let members = ordered[..count].to_vec();
                    if best_group.as_ref().is_none_or(|best| members.len() > best.len()) {
                        best_group = Some(members);
                    }
                }
                if let Some(members) = best_group {
                    let stack_id = self.fresh();
                    self.commit(vec![ProductionOperation::Stack { stack_id: stack_id.clone(),
                            member_ids: members.clone() }], DecisionKind::StackAndCut, members,
                        "Apilado compatible: ahorro de pasadas superior al costo estimado de manejo")?;
                    // The next loop will execute the shared cut, allowing
                    // the existing-stack regrouping decision to be re-evaluated.
                    debug_assert!(self.sim.stacks.iter().any(|s| s.id == stack_id));
                    continue;
                }
            }
            // No economical grouping. Execute the lexicographically first
            // free route (fully deterministic for identical input).
            if let Some(p) = self.sim.workpieces.iter().find(|p|
                p.state == PhysicalState::Active && p.stack_id.is_none()
                    && self.pending(&p.id).is_some()) {
                let id = p.id.clone();
                let next = self.pending(&id).expect("checked");
                self.cut_single(&id, next)?;
                continue;
            }
            return Err(invalid("stalled", "pending routes exist but no physically reachable action"));
        }
        if self.options.release_finished_stacks {
            let stacks = self.sim.stacks.clone();
            for stack in stacks {
                if stack.member_ids.iter().all(|m| self.routes.contains_key(m)
                    && self.pending(m).is_none()) {
                    self.commit(vec![ProductionOperation::Unstack { stack_id: stack.id }],
                        DecisionKind::ReleaseFinished, stack.member_ids,
                        "Liberar franjas terminadas para su clasificación/etiquetado posterior")?;
                }
            }
        }
        Ok(())
    }
}

/// Plan all specified cut routes, considering repeated cuts, package retention,
/// dynamic unstacking and regrouping. Returns a *simulated* operation trace.
/// It is NOT a machine-safe cutting program and makes no global-optimality claim.
pub fn plan_dynamic_stacks(
    model: &CuttingJobModel,
    prefix: &ProductionPlan,
    routes: &[WorkpieceRoute],
    options: DynamicStackOptions,
) -> Result<DynamicStackResult, DynamicStackError> {
    if routes.is_empty() || routes.len() > MAX_ROUTES || options.max_operations == 0
        || options.max_operations > MAX_OPERATIONS {
        return Err(invalid("limits", "routes must contain 1..5000 items and max_operations 1..20000"));
    }
    let base = simulate_production(model, prefix).map_err(DynamicStackError::Simulation)?;
    let lookup: BTreeMap<&str, &PhysicalWorkpiece> = base.workpieces.iter()
        .map(|p| (p.id.as_str(), p)).collect();
    let mut mapped = BTreeMap::new();
    let mut node_total = 0usize;
    let mut raw_cuts = 0u64;
    for r in routes {
        if mapped.contains_key(&r.workpiece_id) {
            return Err(invalid("duplicate_route", &r.workpiece_id));
        }
        let p = lookup.get(r.workpiece_id.as_str())
            .ok_or_else(|| invalid("unknown_workpiece", &r.workpiece_id))?;
        if p.state != PhysicalState::Active || !matches!(p.kind, WorkpieceKind::Strip | WorkpieceKind::Region) {
            return Err(invalid("ineligible_workpiece", &r.workpiece_id));
        }
        let count = r.route.count_cuts();
        node_total = node_total.saturating_add(count.saturating_mul(2).saturating_add(1));
        if node_total > MAX_ROUTE_NODES {
            return Err(invalid("route_limit", "too many cut route nodes"));
        }
        raw_cuts += count as u64;
        mapped.insert(r.workpiece_id.clone(), r.route.clone());
    }
    let mut used_ids = BTreeSet::new();
    for p in &base.workpieces { used_ids.insert(p.id.clone()); }
    for s in &base.stacks { used_ids.insert(s.id.clone()); }
    for e in &base.events {
        match &e.operation {
            ProductionOperation::Stack { stack_id, .. } | ProductionOperation::Unstack { stack_id }
            | ProductionOperation::RotateStack { stack_id } | ProductionOperation::MoveStack { stack_id, .. } => {
                used_ids.insert(stack_id.clone());
            }
            ProductionOperation::CutStack { stack_id, first_stack_id, second_stack_id, .. } => {
                used_ids.insert(stack_id.clone()); used_ids.insert(first_stack_id.clone());
                used_ids.insert(second_stack_id.clone());
            }
            _ => {}
        }
    }
    let mut planner = Planner { model, prefix: prefix.clone(), options, routes: mapped,
        sim: base.clone(), initial_sim: base.clone(), additions: Vec::new(), decisions: Vec::new(),
        used_ids, next_id: 0 };
    planner.run()?;
    let added_passes = planner.sim.summary.saw_passes - planner.initial_sim.summary.saw_passes;
    let planned_saw_passes = added_passes;
    let stack_count = planner.additions.iter().filter(|op| matches!(op, ProductionOperation::Stack { .. })).count() as u128;
    let unstack_count = planner.additions.iter().filter(|op| matches!(op, ProductionOperation::Unstack { .. })).count() as u128;
    let savings = (raw_cuts as i128 - planned_saw_passes as i128)
        * i128::from(options.saw_pass_cost_minor)
        - (stack_count as i128) * i128::from(options.stack_cost_minor)
        - (unstack_count as i128) * i128::from(options.unstack_cost_minor);
    let terminal_ids = planner.routes.iter().filter(|(_, route)| route.next_cut().is_none())
        .map(|(id, _)| id.clone()).collect();
    Ok(DynamicStackResult { decisions: planner.decisions, operations_added: planner.additions,
        base_simulation: planner.initial_sim, projected_simulation: planner.sim,
        standalone_saw_passes: raw_cuts, planned_saw_passes,
        saw_passes_avoided: raw_cuts.saturating_sub(planned_saw_passes),
        estimated_net_savings_minor: savings.to_string(), terminal_workpiece_ids: terminal_ids,
        note: "Stage 13 dynamically maintains, releases and regroups compatible cut regions; all operations pass an independent full simulator replay. Savings use a heuristic per-saw-pass/handling model and do not include real machine clamp/scheduling constraints (Stage 14). Terminal regions are not automatically declared finished parts or scraps.".into() })
}
