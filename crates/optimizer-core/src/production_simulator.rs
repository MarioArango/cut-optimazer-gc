//! Stage 11 — deterministic physical-material simulator.
//!
//! Explicit instructions drive state changes; this is NOT yet an automatic
//! production scheduler or a certified machine program. The simulator proves
//! stock ownership, parent-child lineage, local saw geometry, stack limits,
//! compatible operations, and physical material conservation at every step.
//! Registration/clamp reachability, guillotine-stage scheduling, and machine-
//! specific handling are deliberately postponed to stage 14.

use crate::geometry::Rect;
use crate::model::{
    validate_model, Axis, CutDirection, CuttingJobModel, Orientation,
    RegistrationEdge, StockKind, Unit, WorkpieceKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const MAX_INITIAL_BOARDS: usize = 5_000;
const MAX_OPERATIONS: usize = 20_000;
const MAX_WORKPIECES: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialBoard {
    pub board_id: String,
    pub stock_id: String,
    pub stock_unit_index: u32,
    #[serde(default)]
    pub rotated_90: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductionOperation {
    Stack { stack_id: String, member_ids: Vec<String> },
    Unstack { stack_id: String },
    CutWorkpiece {
        workpiece_id: String,
        direction: CutDirection,
        at: Unit,
        first_id: String,
        second_id: String,
    },
    CutStack {
        stack_id: String,
        direction: CutDirection,
        at: Unit,
        first_stack_id: String,
        second_stack_id: String,
        first_member_ids: Vec<String>,
        second_member_ids: Vec<String>,
    },
    RotateWorkpiece { workpiece_id: String },
    RotateStack { stack_id: String },
    MoveWorkpiece { workpiece_id: String, station: String },
    MoveStack { stack_id: String, station: String },
    Finish { workpiece_id: String, part_id: String, rotated: bool },
    Scrap { workpiece_id: String, reusable: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionPlan {
    pub initial_boards: Vec<InitialBoard>,
    pub operations: Vec<ProductionOperation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalState { Active, Consumed, Finished, Scrap }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalWorkpiece {
    pub id: String,
    pub origin_board_id: String,
    pub origin_stock_id: String,
    pub origin_stock_unit_index: u32,
    pub parent_id: Option<String>,
    pub kind: WorkpieceKind,
    pub material_id: String,
    pub width: Unit,
    pub height: Unit,
    pub thickness: Unit,
    pub grain_axis: Option<Axis>,
    pub orientation: Orientation,
    pub registration_edge: RegistrationEdge,
    pub station: String,
    pub state: PhysicalState,
    pub stack_id: Option<String>,
    /// Local coordinates in the parent's frame at the moment of cutting.
    pub source_rect: Rect,
    pub finished_part_id: Option<String>,
    pub reusable_scrap: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalStack {
    pub id: String,
    /// Bottom-to-top member ordering.
    pub member_ids: Vec<String>,
    pub total_height: u64,
    pub station: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionEvent {
    pub step: usize,
    pub action: String,
    /// Original operation, including exact references and transformation params.
    pub operation: ProductionOperation,
    pub consumed_workpiece_ids: Vec<String>,
    pub created_workpiece_ids: Vec<String>,
    pub saw_passes: u32,
    /// Exact square tenths-of-millimetre units, serialized as decimal string.
    pub kerf_area_sq_units: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionSummary {
    pub initial_boards: usize,
    pub operations_executed: usize,
    pub saw_passes: u64,
    pub live_workpieces: usize,
    pub active_stacks: usize,
    pub finished_workpieces: usize,
    pub scrap_workpieces: usize,
    pub finished_per_part: BTreeMap<String, u64>,
    pub initial_area_sq_units: String,
    pub accounted_area_sq_units: String,
    pub kerf_area_sq_units: String,
    pub area_conserved: bool,
    pub production_certified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionSimulation {
    pub workpieces: Vec<PhysicalWorkpiece>,
    pub stacks: Vec<PhysicalStack>,
    pub events: Vec<ProductionEvent>,
    pub summary: ProductionSummary,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimulationError {
    InvalidModel(Vec<String>),
    InvalidPlan { step: Option<usize>, code: String, message: String },
}
impl fmt::Display for SimulationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModel(issues) => write!(f, "invalid model: {}", issues.join("; ")),
            Self::InvalidPlan { step, code, message } => match step {
                Some(s) => write!(f, "production operation {s} [{code}]: {message}"),
                None => write!(f, "production plan [{code}]: {message}"),
            },
        }
    }
}
impl std::error::Error for SimulationError {}

fn invalid(step: Option<usize>, code: &str, message: impl Into<String>) -> SimulationError {
    SimulationError::InvalidPlan { step, code: code.into(), message: message.into() }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b'.')
}
fn valid_station(id: &str) -> bool { valid_id(id) }
fn rotated_axis(axis: Option<Axis>) -> Option<Axis> {
    axis.map(|a| match a { Axis::X => Axis::Y, Axis::Y => Axis::X })
}
fn turned_orientation(o: Orientation) -> Orientation {
    match o { Orientation::Deg0 => Orientation::Deg90, Orientation::Deg90 => Orientation::Deg0 }
}
fn turned_edge(e: RegistrationEdge) -> RegistrationEdge {
    match e {
        RegistrationEdge::Left => RegistrationEdge::Front,
        RegistrationEdge::Front => RegistrationEdge::Right,
        RegistrationEdge::Right => RegistrationEdge::Back,
        RegistrationEdge::Back => RegistrationEdge::Left,
    }
}
fn area(p: &PhysicalWorkpiece) -> u128 { u128::from(p.width) * u128::from(p.height) }

struct Simulator<'a> {
    model: &'a CuttingJobModel,
    pieces: BTreeMap<String, PhysicalWorkpiece>,
    stacks: BTreeMap<String, PhysicalStack>,
    used_stack_ids: BTreeSet<String>,
    events: Vec<ProductionEvent>,
    saw_passes: u64,
    kerf_area: u128,
    initial_area: u128,
    finished_per_part: BTreeMap<String, u64>,
}

impl<'a> Simulator<'a> {
    fn new(model: &'a CuttingJobModel) -> Self {
        Self { model, pieces: BTreeMap::new(), stacks: BTreeMap::new(),
            used_stack_ids: BTreeSet::new(), events: Vec::new(), saw_passes: 0,
            kerf_area: 0, initial_area: 0, finished_per_part: BTreeMap::new() }
    }
    fn piece(&self, id: &str, step: usize, require_free: bool) -> Result<PhysicalWorkpiece, SimulationError> {
        let p = self.pieces.get(id).ok_or_else(|| invalid(Some(step), "unknown_workpiece", format!("workpiece '{id}' does not exist")))?;
        if p.state != PhysicalState::Active {
            return Err(invalid(Some(step), "unavailable_workpiece", format!("'{id}' is not active")));
        }
        if require_free && p.stack_id.is_some() {
            return Err(invalid(Some(step), "stacked_workpiece", format!("'{id}' must first be unstacked")));
        }
        Ok(p.clone())
    }
    fn stack(&self, id: &str, step: usize) -> Result<PhysicalStack, SimulationError> {
        self.stacks.get(id).cloned().ok_or_else(|| invalid(Some(step), "unknown_stack", format!("stack '{id}' does not exist")))
    }
    fn new_piece(&mut self, p: PhysicalWorkpiece, step: usize) -> Result<(), SimulationError> {
        if !valid_id(&p.id) || self.pieces.contains_key(&p.id) || self.pieces.len() >= MAX_WORKPIECES {
            return Err(invalid(Some(step), "duplicate_or_invalid_id", format!("invalid/reused/excessive workpiece ID '{}'", p.id)));
        }
        self.pieces.insert(p.id.clone(), p);
        Ok(())
    }
    fn register_stack(&mut self, id: &str, members: &[String], step: usize) -> Result<(), SimulationError> {
        if !valid_id(id) || self.used_stack_ids.contains(id) {
            return Err(invalid(Some(step), "duplicate_or_invalid_stack_id", id));
        }
        if members.len() < 2 || members.len() > usize::try_from(self.model.machine.max_stack_members).unwrap_or(usize::MAX) {
            return Err(invalid(Some(step), "stack_members", "package must contain at least 2 and at most max_stack_members"));
        }
        let mut ids = BTreeSet::new();
        let mut parts = Vec::with_capacity(members.len());
        for member in members {
            if !ids.insert(member) { return Err(invalid(Some(step), "duplicate_member", member.as_str())); }
            parts.push(self.piece(member, step, true)?);
        }
        let first = &parts[0];
        let all_boards = parts.iter().all(|p| p.kind == WorkpieceKind::Board);
        if all_boards {
            if !self.model.machine.stacking.full_boards {
                return Err(invalid(Some(step), "stack_capability", "machine cannot stack full boards"));
            }
        } else if !self.model.machine.stacking.strips || !self.model.machine.stacking.restacking {
            return Err(invalid(Some(step), "stack_capability", "machine profile does not authorize assembling cut regions into stacks"));
        }
        let height: u64 = parts.iter().map(|p| u64::from(p.thickness)).sum();
        if height > u64::from(self.model.machine.max_stack_height) {
            return Err(invalid(Some(step), "stack_height", format!("package height {height} exceeds machine limit")));
        }
        if parts.iter().any(|p| p.material_id != first.material_id || p.thickness != first.thickness
            || p.width != first.width || p.height != first.height
            || p.grain_axis != first.grain_axis || p.orientation != first.orientation
            || p.registration_edge != first.registration_edge || p.station != first.station) {
            return Err(invalid(Some(step), "incompatible_stack", "stack members must have matching dimensions, material, grain, orientation, registration and station"));
        }
        // Every member points to the same physical package.
        for member in members { self.pieces.get_mut(member).expect("checked").stack_id = Some(id.to_owned()); }
        self.used_stack_ids.insert(id.to_owned());
        self.stacks.insert(id.to_owned(), PhysicalStack {
            id: id.to_owned(), member_ids: members.to_vec(), total_height: height,
            station: first.station.clone(),
        });
        Ok(())
    }
    fn take_stack(&mut self, id: &str, step: usize) -> Result<PhysicalStack, SimulationError> {
        let s = self.stack(id, step)?;
        self.stacks.remove(id);
        for m in &s.member_ids { self.pieces.get_mut(m).expect("stack members exist").stack_id = None; }
        Ok(s)
    }
    fn cut_geometry(&self, p: &PhysicalWorkpiece, direction: CutDirection, at: Unit, step: usize)
        -> Result<crate::geometry::SplitResult, SimulationError> {
        let saw_len = match direction { CutDirection::Vertical => p.height, CutDirection::Horizontal => p.width };
        if saw_len > self.model.machine.max_cut_length {
            return Err(invalid(Some(step), "cut_length", "saw path exceeds max_cut_length"));
        }
        Rect::new(0, 0, p.width, p.height)
            .and_then(|rect| rect.split(direction, at, self.model.machine.kerf))
            .map_err(|e| invalid(Some(step), "cut_geometry", e.to_string()))
    }
    fn child(&self, parent: &PhysicalWorkpiece, id: &str, rect: Rect) -> PhysicalWorkpiece {
        PhysicalWorkpiece {
            id: id.into(), origin_board_id: parent.origin_board_id.clone(),
            origin_stock_id: parent.origin_stock_id.clone(),
            origin_stock_unit_index: parent.origin_stock_unit_index,
            parent_id: Some(parent.id.clone()), kind: if parent.kind == WorkpieceKind::Board { WorkpieceKind::Strip } else { WorkpieceKind::Region },
            material_id: parent.material_id.clone(), width: rect.width, height: rect.height,
            thickness: parent.thickness, grain_axis: parent.grain_axis,
            orientation: parent.orientation, registration_edge: parent.registration_edge,
            station: parent.station.clone(), state: PhysicalState::Active, stack_id: None,
            source_rect: rect, finished_part_id: None, reusable_scrap: None,
        }
    }
    fn consume(&mut self, id: &str) { self.pieces.get_mut(id).expect("checked").state = PhysicalState::Consumed; }
    fn rotate_one(&mut self, id: &str) {
        let p = self.pieces.get_mut(id).expect("checked");
        std::mem::swap(&mut p.width, &mut p.height);
        p.grain_axis = rotated_axis(p.grain_axis);
        p.orientation = turned_orientation(p.orientation);
        p.registration_edge = turned_edge(p.registration_edge);
    }
    fn event(&mut self, step: usize, operation: &ProductionOperation, action: &str, consumed: Vec<String>, created: Vec<String>, passes: u32, kerf: u128) {
        self.saw_passes += u64::from(passes);
        self.kerf_area += kerf;
        self.events.push(ProductionEvent { step, action: action.into(), operation: operation.clone(), consumed_workpiece_ids: consumed,
            created_workpiece_ids: created, saw_passes: passes, kerf_area_sq_units: kerf.to_string() });
    }
    fn apply(&mut self, step: usize, op: &ProductionOperation) -> Result<(), SimulationError> {
        use ProductionOperation::*;
        match op {
            Stack { stack_id, member_ids } => {
                self.register_stack(stack_id, member_ids, step)?;
                self.event(step, op, "stack", Vec::new(), Vec::new(), 0, 0);
            }
            Unstack { stack_id } => {
                self.take_stack(stack_id, step)?;
                self.event(step, op, "unstack", Vec::new(), Vec::new(), 0, 0);
            }
            CutWorkpiece { workpiece_id, direction, at, first_id, second_id } => {
                let p = self.piece(workpiece_id, step, true)?;
                if first_id == second_id || !valid_id(first_id) || !valid_id(second_id)
                    || self.pieces.contains_key(first_id) || self.pieces.contains_key(second_id) {
                    return Err(invalid(Some(step), "duplicate_or_invalid_id", "cut child IDs must be unique and unused"));
                }
                let split = self.cut_geometry(&p, *direction, *at, step)?;
                let kerf = split.saw_path.area_sq_units();
                self.new_piece(self.child(&p, first_id, split.first), step)?;
                self.new_piece(self.child(&p, second_id, split.second), step)?;
                self.consume(workpiece_id);
                self.event(step, op, "cut_workpiece", vec![workpiece_id.clone()],
                    vec![first_id.clone(), second_id.clone()], 1, kerf);
            }
            CutStack { stack_id, direction, at, first_stack_id, second_stack_id, first_member_ids, second_member_ids } => {
                let stack = self.stack(stack_id, step)?;
                let n = stack.member_ids.len();
                if first_stack_id == second_stack_id || !valid_id(first_stack_id) || !valid_id(second_stack_id)
                    || self.used_stack_ids.contains(first_stack_id) || self.used_stack_ids.contains(second_stack_id) {
                    return Err(invalid(Some(step), "duplicate_or_invalid_stack_id", "new stack IDs must be distinct and unused"));
                }
                if first_member_ids.len() != n || second_member_ids.len() != n {
                    return Err(invalid(Some(step), "cut_layers", "each output stack must retain one region per input layer"));
                }
                let child_ids: Vec<&String> = first_member_ids.iter().chain(second_member_ids.iter()).collect();
                let mut seen = BTreeSet::new();
                for id in &child_ids {
                    if !valid_id(id) || !seen.insert(id.as_str()) || self.pieces.contains_key(id.as_str()) {
                        return Err(invalid(Some(step), "duplicate_or_invalid_id", "output region IDs must be unique and unused"));
                    }
                }
                let parents: Vec<_> = stack.member_ids.iter().map(|id| self.piece(id, step, false)).collect::<Result<_,_>>()?;
                let split = self.cut_geometry(&parents[0], *direction, *at, step)?;
                for p in &parents[1..] {
                    let other = self.cut_geometry(p, *direction, *at, step)?;
                    if other != split { return Err(invalid(Some(step), "stack_cut_mismatch", "stack members must share identical cut geometry")); }
                }
                self.take_stack(stack_id, step)?;
                let mut kerf_area = 0_u128;
                for (i, parent) in parents.iter().enumerate() {
                    self.new_piece(self.child(parent, &first_member_ids[i], split.first), step)?;
                    self.new_piece(self.child(parent, &second_member_ids[i], split.second), step)?;
                    self.consume(&parent.id);
                    kerf_area += split.saw_path.area_sq_units();
                }
                self.register_stack(first_stack_id, first_member_ids, step)?;
                self.register_stack(second_stack_id, second_member_ids, step)?;
                self.event(step, op, "cut_stack", stack.member_ids, child_ids.iter().map(|id| id.as_str().to_owned()).collect(), 1, kerf_area);
            }
            RotateWorkpiece { workpiece_id } => {
                if !self.model.machine.allow_board_rotation { return Err(invalid(Some(step), "rotation_disabled", "machine does not allow rotations")); }
                self.piece(workpiece_id, step, true)?;
                self.rotate_one(workpiece_id);
                self.event(step, op, "rotate_workpiece", Vec::new(), Vec::new(), 0, 0);
            }
            RotateStack { stack_id } => {
                if !self.model.machine.allow_board_rotation { return Err(invalid(Some(step), "rotation_disabled", "machine does not allow rotations")); }
                let s = self.stack(stack_id, step)?;
                for id in &s.member_ids { self.rotate_one(id); }
                self.event(step, op, "rotate_stack", Vec::new(), Vec::new(), 0, 0);
            }
            MoveWorkpiece { workpiece_id, station } => {
                if !valid_station(station) { return Err(invalid(Some(step), "invalid_station", station.as_str())); }
                self.piece(workpiece_id, step, true)?;
                self.pieces.get_mut(workpiece_id).expect("checked").station = station.clone();
                self.event(step, op, "move_workpiece", Vec::new(), Vec::new(), 0, 0);
            }
            MoveStack { stack_id, station } => {
                if !valid_station(station) { return Err(invalid(Some(step), "invalid_station", station.as_str())); }
                let s = self.stack(stack_id, step)?;
                self.stacks.get_mut(stack_id).expect("checked").station = station.clone();
                for id in &s.member_ids { self.pieces.get_mut(id).expect("checked").station = station.clone(); }
                self.event(step, op, "move_stack", Vec::new(), Vec::new(), 0, 0);
            }
            Finish { workpiece_id, part_id, rotated } => {
                let p = self.piece(workpiece_id, step, true)?;
                let part = self.model.parts.iter().find(|part| part.id == *part_id)
                    .ok_or_else(|| invalid(Some(step), "unknown_part", part_id.as_str()))?;
                let expected_dimensions = if *rotated { (part.height, part.width) } else { (part.width, part.height) };
                let expected_grain = if *rotated { rotated_axis(part.grain_axis) } else { part.grain_axis };
                if p.material_id != part.material_id ||
                    (p.width, p.height) != expected_dimensions ||
                    (*rotated && !part.allow_rotate_90) ||
                    (part.grain_axis.is_some() && p.grain_axis != expected_grain) {
                    return Err(invalid(Some(step), "finished_part_mismatch", format!("workpiece '{workpiece_id}' cannot fulfill part '{part_id}'")));
                }
                let count = self.finished_per_part.get(part_id).copied().unwrap_or(0);
                if count >= u64::from(part.quantity) { return Err(invalid(Some(step), "overproduction", part_id.as_str())); }
                self.finished_per_part.insert(part_id.clone(), count + 1);
                let target = self.pieces.get_mut(workpiece_id).expect("checked");
                target.state = PhysicalState::Finished;
                target.finished_part_id = Some(part_id.clone());
                self.event(step, op, "finish", Vec::new(), Vec::new(), 0, 0);
            }
            Scrap { workpiece_id, reusable } => {
                self.piece(workpiece_id, step, true)?;
                let p = self.pieces.get_mut(workpiece_id).expect("checked");
                p.state = PhysicalState::Scrap;
                p.reusable_scrap = Some(*reusable);
                self.event(step, op, "scrap", Vec::new(), Vec::new(), 0, 0);
            }
        }
        // Catch internal ledger mistakes at the exact operation that caused them.
        if !self.conserved() { return Err(invalid(Some(step), "material_balance", "material area is not conserved")); }
        Ok(())
    }
    fn accounted_area(&self) -> u128 {
        self.pieces.values().filter(|p| p.state != PhysicalState::Consumed).map(area).sum::<u128>() + self.kerf_area
    }
    fn conserved(&self) -> bool { self.accounted_area() == self.initial_area }
    fn finish(self, initial_boards: usize) -> ProductionSimulation {
        let accounted = self.accounted_area();
        let live = self.pieces.values().filter(|p| p.state == PhysicalState::Active).count();
        let finished = self.pieces.values().filter(|p| p.state == PhysicalState::Finished).count();
        let scrap = self.pieces.values().filter(|p| p.state == PhysicalState::Scrap).count();
        ProductionSimulation {
            summary: ProductionSummary {
                initial_boards, operations_executed: self.events.len(),
                saw_passes: self.saw_passes, live_workpieces: live,
                active_stacks: self.stacks.len(), finished_workpieces: finished,
                scrap_workpieces: scrap, finished_per_part: self.finished_per_part,
                initial_area_sq_units: self.initial_area.to_string(),
                accounted_area_sq_units: accounted.to_string(),
                kerf_area_sq_units: self.kerf_area.to_string(), area_conserved: accounted == self.initial_area,
                production_certified: false,
            },
            workpieces: self.pieces.into_values().collect(), stacks: self.stacks.into_values().collect(),
            events: self.events,
            note: "Stage 11 simulates explicit physical operations, per-layer genealogy, stack eligibility and material balance. It does not generate or certify a CNC/seccionadora schedule; clamp reachability, cut-stage order, trim preparation and dynamic stack optimization are future stages.".into(),
        }
    }
}


/// Run a fully deterministic sequence of proposed physical operations.
/// Initial boards are FULL physical stock rectangles (trim must be cut explicitly);
/// staged trimming is not silently applied. A proposed plan is rejected at the
/// first violated constraint; no partial result is emitted on failure.
pub fn simulate_production(model: &CuttingJobModel, plan: &ProductionPlan)
    -> Result<ProductionSimulation, SimulationError> {
    validate_model(model).map_err(|issues| SimulationError::InvalidModel(
        issues.into_iter().map(|i| format!("{}: {}", i.path, i.code)).collect()))?;
    if plan.initial_boards.is_empty() || plan.initial_boards.len() > MAX_INITIAL_BOARDS
        || plan.operations.len() > MAX_OPERATIONS {
        return Err(invalid(None, "plan_limits", "must provide 1..5000 initial boards and <=20000 operations"));
    }
    let mut sim = Simulator::new(model);
    let mut stock_used = BTreeSet::new();
    for board in &plan.initial_boards {
        if !valid_id(&board.board_id) || sim.pieces.contains_key(&board.board_id) {
            return Err(invalid(None, "duplicate_board_id", board.board_id.as_str()));
        }
        let stock = model.stocks.iter().find(|s| s.id == board.stock_id)
            .ok_or_else(|| invalid(None, "unknown_stock", board.stock_id.as_str()))?;
        if board.stock_unit_index >= stock.quantity {
            return Err(invalid(None, "stock_index", format!("{} index {} unavailable", stock.id, board.stock_unit_index)));
        }
        if !stock_used.insert((board.stock_id.clone(), board.stock_unit_index)) {
            return Err(invalid(None, "duplicate_stock_unit", "same physical stock unit used more than once"));
        }
        if board.rotated_90 && !model.machine.allow_board_rotation {
            return Err(invalid(None, "rotation_disabled", board.board_id.as_str()));
        }
        let (width, height, grain_axis, orientation) = if board.rotated_90 {
            (stock.height, stock.width, rotated_axis(stock.grain_axis), Orientation::Deg90)
        } else {
            (stock.width, stock.height, stock.grain_axis, Orientation::Deg0)
        };
        let rect = Rect::new(0, 0, width, height).map_err(|e| invalid(None, "board_geometry", e.to_string()))?;
        sim.initial_area += rect.area_sq_units();
        sim.pieces.insert(board.board_id.clone(), PhysicalWorkpiece {
            id: board.board_id.clone(), origin_board_id: board.board_id.clone(),
            origin_stock_id: board.stock_id.clone(), origin_stock_unit_index: board.stock_unit_index,
            parent_id: None, kind: if stock.kind == StockKind::FullBoard { WorkpieceKind::Board } else { WorkpieceKind::Region },
            material_id: stock.material_id.clone(), width, height, thickness: stock.thickness,
            grain_axis, orientation, registration_edge: if board.rotated_90 { RegistrationEdge::Front } else { RegistrationEdge::Left },
            station: "feed".into(), state: PhysicalState::Active, stack_id: None,
            source_rect: rect, finished_part_id: None, reusable_scrap: None,
        });
    }
    for (i, op) in plan.operations.iter().enumerate() { sim.apply(i + 1, op)?; }
    Ok(sim.finish(plan.initial_boards.len()))
}
