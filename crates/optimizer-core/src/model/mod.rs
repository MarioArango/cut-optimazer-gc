//! Stage 2 public industrial cutting data model.
//!
//! All dimensions are non-negative integer tenths of a millimeter (`Unit`).
//! Cutting geometry, physical sequence validation, and optimization are NOT
//! implemented at this stage. Structural and reference validation lives in
//! `validation.rs`.

pub mod validation;

use serde::{Deserialize, Serialize};

/// One tenth of a millimeter: 2440 mm = 24_400 units.
pub type Unit = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutDirection {
    Vertical,
    Horizontal,
}

/// Guillotine cutting pattern: the direction of each cutting level, always
/// alternating. A tree may finish a branch before the last level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StagePattern {
    #[serde(rename = "V-H")]
    VH,
    #[serde(rename = "H-V")]
    HV,
    #[serde(rename = "V-H-V")]
    VHV,
    #[serde(rename = "H-V-H")]
    HVH,
    #[serde(rename = "V-H-V-H")]
    VHVH,
    #[serde(rename = "H-V-H-V")]
    HVHV,
    #[serde(rename = "V-H-V-H-V")]
    VHVHV,
    #[serde(rename = "H-V-H-V-H")]
    HVHVH,
    #[serde(rename = "V-H-V-H-V-H")]
    VHVHVH,
    #[serde(rename = "H-V-H-V-H-V")]
    HVHVHV,
}

/// Deepest supported guillotine pattern.
pub const MAX_CUT_LEVELS: u8 = 6;

impl StagePattern {
    pub const ALL: [StagePattern; 10] = [
        Self::VH, Self::HV, Self::VHV, Self::HVH, Self::VHVH, Self::HVHV,
        Self::VHVHV, Self::HVHVH, Self::VHVHVH, Self::HVHVHV,
    ];

    /// Cut direction of every level, first level first.
    pub fn directions(self) -> &'static [CutDirection] {
        use CutDirection::{Horizontal as H, Vertical as V};
        const VS: [CutDirection; 6] = [V, H, V, H, V, H];
        const HS: [CutDirection; 6] = [H, V, H, V, H, V];
        let levels = usize::from(self.levels());
        match self.first() {
            V => &VS[..levels],
            H => &HS[..levels],
        }
    }

    /// Number of cutting levels (2..=6).
    pub fn levels(self) -> u8 {
        match self {
            Self::VH | Self::HV => 2,
            Self::VHV | Self::HVH => 3,
            Self::VHVH | Self::HVHV => 4,
            Self::VHVHV | Self::HVHVH => 5,
            Self::VHVHVH | Self::HVHVHV => 6,
        }
    }

    /// Direction of the first-level cuts.
    pub fn first(self) -> CutDirection {
        match self {
            Self::VH | Self::VHV | Self::VHVH | Self::VHVHV | Self::VHVHVH => CutDirection::Vertical,
            _ => CutDirection::Horizontal,
        }
    }

    /// The pattern with `levels` levels starting with `first`, if supported.
    pub fn from_levels(first: CutDirection, levels: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.first() == first && p.levels() == levels)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockKind {
    FullBoard,
    Remnant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkpieceKind {
    Board,
    Strip,
    Region,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StackKind {
    FullBoard,
    Strip,
    MixedRegion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[serde(rename = "0")]
    Deg0,
    #[serde(rename = "90")]
    Deg90,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationEdge {
    Left,
    Right,
    Front,
    Back,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub id: String,
    pub name: String,
    pub thickness: Unit,
    /// Whether the material imposes a grain constraint on stock and parts.
    pub directional_grain: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub id: String,
    pub material_id: String,
    pub width: Unit,
    pub height: Unit,
    pub quantity: u32,
    pub allow_rotate_90: bool,
    /// Grain axis in the piece's local coordinate system, when applicable.
    pub grain_axis: Option<Axis>,
    #[serde(default)]
    pub priority: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrimMargins {
    #[serde(default)]
    pub top: Unit,
    #[serde(default)]
    pub right: Unit,
    #[serde(default)]
    pub bottom: Unit,
    #[serde(default)]
    pub left: Unit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stock {
    pub id: String,
    pub material_id: String,
    pub kind: StockKind,
    pub width: Unit,
    pub height: Unit,
    pub thickness: Unit,
    pub quantity: u32,
    pub grain_axis: Option<Axis>,
    #[serde(default)]
    pub trim: TrimMargins,
    /// Minor units of local currency, e.g. PEN cents. Never floating-point.
    pub unit_cost_minor: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackingCapabilities {
    pub full_boards: bool,
    pub strips: bool,
    pub restacking: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineProfile {
    pub id: String,
    pub kerf: Unit,
    pub max_cut_length: Unit,
    pub min_piece_width: Unit,
    pub min_piece_height: Unit,
    pub min_strip_width: Unit,
    pub min_clamp_width: Unit,
    pub max_stack_height: Unit,
    pub max_stack_members: u32,
    pub allow_board_rotation: bool,
    pub allow_recuts: bool,
    pub allowed_patterns: Vec<StagePattern>,
    pub stacking: StackingCapabilities,
}

/// Tree shape only. Stage 5 will implement the geometry and feasibility
/// semantics of the cuts, including saw kerf and stage transitions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatternNode {
    Split {
        node_id: String,
        direction: CutDirection,
        /// Position relative to the region's origin in tenths of mm.
        at: Unit,
        first: Box<PatternNode>,
        second: Box<PatternNode>,
    },
    Piece {
        node_id: String,
        part_id: String,
        rotated: bool,
    },
    Residual {
        node_id: String,
        reusable: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuttingPattern {
    pub id: String,
    pub stock_id: String,
    pub mode: StagePattern,
    pub root: PatternNode,
}

/// A physical piece of stock or an intermediate part of a cut board.
/// `origin_board_key` identifies one concrete original sheet within stock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workpiece {
    pub id: String,
    pub origin_stock_id: String,
    pub origin_board_key: String,
    pub parent_workpiece_id: Option<String>,
    pub kind: WorkpieceKind,
    pub material_id: String,
    pub width: Unit,
    pub height: Unit,
    pub thickness: Unit,
    pub grain_axis: Option<Axis>,
    pub orientation: Orientation,
    pub registration_edge: RegistrationEdge,
    pub current_stage: u8,
    pub pattern_id: Option<String>,
    pub pending_node_id: Option<String>,
}

/// Ordered physical stack. The sequence in `member_ids` is bottom -> top.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuttingStack {
    pub id: String,
    pub kind: StackKind,
    pub member_ids: Vec<String>,
    pub orientation: Orientation,
    pub registration_edge: RegistrationEdge,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuttingJobModel {
    pub materials: Vec<Material>,
    pub parts: Vec<Part>,
    pub stocks: Vec<Stock>,
    pub machine: MachineProfile,
    #[serde(default)]
    pub patterns: Vec<CuttingPattern>,
    #[serde(default)]
    pub workpieces: Vec<Workpiece>,
    #[serde(default)]
    pub stacks: Vec<CuttingStack>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSummary {
    pub material_types: usize,
    pub part_types: usize,
    pub requested_pieces: u64,
    pub stock_types: usize,
    pub available_boards: u64,
    pub pattern_templates: usize,
    pub tracked_workpieces: usize,
    pub stacks: usize,
    pub stacked_workpieces: usize,
    /// Structural correctness is NOT geometry or production feasibility.
    pub geometry_validated: bool,
    pub production_validated: bool,
}

pub use validation::{validate_model, ModelIssue};
