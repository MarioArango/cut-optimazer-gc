//! Stage 5 — deterministic evaluation of a supplied guillotine cutting tree.
//!
//! A *geometric* depth-first traversal, NOT a machine execution schedule.
//! At every split the saw channel is removed by `Rect::split`, all dimensions
//! are in integer tenths of millimetres, and cuts on one root-to-leaf path
//! must follow the configured V-H / H-V / V-H-V / H-V-H direction sequence.
//! Node stages can be repeated; a leaf may end before the last allowed stage.
//!
//! This module does not generate layouts, determine full-job quantities,
//! or prove clamp/strip/stack feasibility (later stages 6, 10–14).

use crate::geometry::Rect;
use crate::model::{
    validate_model, Axis, CutDirection, CuttingJobModel, Part,
    PatternNode, StagePattern, TrimMargins, Unit,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;

/// Binary-tree depth: a level with n parallel cuts adds n, so six levels of
/// dense cuts need more than the original 128.
const MAX_TREE_DEPTH: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeError {
    pub path: String,
    pub code: String,
    pub message: String,
}
impl TreeError {
    fn new(path: impl Into<String>, code: &str, message: impl Into<String>) -> Self {
        Self { path: path.into(), code: code.into(), message: message.into() }
    }
}
impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]: {}", self.path, self.code, self.message)
    }
}
impl std::error::Error for TreeError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluatedCut {
    pub node_id: String,
    /// Geometric stage number along this branch, starting at 1.
    pub stage: u8,
    pub direction: CutDirection,
    pub parent: Rect,
    pub saw_path: Rect,
    pub first: Rect,
    pub second: Rect,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedPiece {
    pub node_id: String,
    pub part_id: String,
    pub rotated: bool,
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResidualRegion {
    pub node_id: String,
    pub reusable: bool,
    pub rect: Rect,
}

/// Spatially explicit tree; split children are in first=left/top,
/// second=right/bottom order. The sawn channel has no output child.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvaluatedNode {
    Split {
        node_id: String,
        rect: Rect,
        stage: u8,
        direction: CutDirection,
        saw_path: Rect,
        first: Box<EvaluatedNode>,
        second: Box<EvaluatedNode>,
    },
    Piece {
        node_id: String,
        rect: Rect,
        part_id: String,
        rotated: bool,
    },
    Residual {
        node_id: String,
        rect: Rect,
        reusable: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeSummary {
    pub cuts: usize,
    pub placed_pieces: usize,
    pub residual_regions: usize,
    pub stages_used: u8,
    /// Exact strings: square (0.1 mm) units, avoids JSON/JS precision loss.
    pub usable_area_sq_units: String,
    pub piece_area_sq_units: String,
    pub residual_area_sq_units: String,
    pub kerf_area_sq_units: String,
    /// A single pattern instance, not a commitment of entire order quantities.
    pub per_part_counts: BTreeMap<String, u32>,
    /// Does NOT imply production/stack feasibility.
    pub production_validated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluatedPattern {
    pub pattern_id: String,
    pub stock_id: String,
    pub board_rotated_90: bool,
    pub mode: StagePattern,
    pub usable_rect: Rect,
    pub root: EvaluatedNode,
    /// DFS order, not the industrial cutting sequence.
    pub geometric_cuts: Vec<EvaluatedCut>,
    pub pieces: Vec<PlacedPiece>,
    pub residuals: Vec<ResidualRegion>,
    pub summary: TreeSummary,
}

fn direction_sequence(mode: StagePattern) -> &'static [CutDirection] {
    mode.directions()
}
fn rotated_axis(axis: Option<Axis>, rotate: bool) -> Option<Axis> {
    if !rotate { return axis; }
    axis.map(|a| match a { Axis::X => Axis::Y, Axis::Y => Axis::X })
}
fn rotated_margins(m: TrimMargins, rotate: bool) -> TrimMargins {
    if !rotate { return m; }
    TrimMargins { top: m.left, right: m.top, bottom: m.right, left: m.bottom }
}

struct EvaluationContext<'a> {
    model: &'a CuttingJobModel,
    parts: HashMap<&'a str, &'a Part>,
    stock_grain: Option<Axis>,
    sequence: &'static [CutDirection],
    cuts: Vec<EvaluatedCut>,
    pieces: Vec<PlacedPiece>,
    residuals: Vec<ResidualRegion>,
    counts: BTreeMap<String, u32>,
    kerf_area: u128,
    piece_area: u128,
    residual_area: u128,
    max_stage: u8,
}

impl EvaluationContext<'_> {
    fn visit(
        &mut self,
        node: &PatternNode,
        rect: Rect,
        prior_stage: Option<usize>,
        path: &str,
        depth: usize,
    ) -> Result<EvaluatedNode, TreeError> {
        if depth > MAX_TREE_DEPTH {
            return Err(TreeError::new(path, "tree_depth_exceeded", "tree is more than 512 nodes deep"));
        }
        match node {
            PatternNode::Split { node_id, direction, at, first, second } => {
                let stage = match prior_stage {
                    None => 0,
                    Some(index) if self.sequence[index] == *direction => index,
                    Some(index) => index + 1,
                };
                if stage >= self.sequence.len() || self.sequence[stage] != *direction {
                    return Err(TreeError::new(path, "invalid_stage_transition",
                        format!("cut {:?} cannot follow this branch of the configured cutting pattern", direction)));
                }
                let cut_length = match direction {
                    CutDirection::Vertical => rect.height,
                    CutDirection::Horizontal => rect.width,
                };
                if cut_length > self.model.machine.max_cut_length {
                    return Err(TreeError::new(path, "cut_too_long",
                        format!("cut length {cut_length} exceeds machine max_cut_length {}", self.model.machine.max_cut_length)));
                }
                let split = rect.split(*direction, *at, self.model.machine.kerf)
                    .map_err(|error| TreeError::new(path, "invalid_split", error.to_string()))?;
                self.kerf_area += split.saw_path.area_sq_units();
                self.max_stage = self.max_stage.max((stage + 1) as u8);
                self.cuts.push(EvaluatedCut {
                    node_id: node_id.clone(), stage: (stage + 1) as u8,
                    direction: *direction, parent: rect,
                    saw_path: split.saw_path, first: split.first, second: split.second,
                });
                let evaluated_first = self.visit(first, split.first, Some(stage), &format!("{path}.first"), depth + 1)?;
                let evaluated_second = self.visit(second, split.second, Some(stage), &format!("{path}.second"), depth + 1)?;
                Ok(EvaluatedNode::Split {
                    node_id: node_id.clone(), rect, stage: (stage + 1) as u8,
                    direction: *direction, saw_path: split.saw_path,
                    first: Box::new(evaluated_first), second: Box::new(evaluated_second),
                })
            }
            PatternNode::Piece { node_id, part_id, rotated } => {
                let part = self.parts.get(part_id.as_str()).ok_or_else(||
                    TreeError::new(path, "unknown_part", format!("part '{part_id}' not found")))?;
                if *rotated && !part.allow_rotate_90 {
                    return Err(TreeError::new(path, "rotation_forbidden", format!("part '{part_id}' cannot rotate")));
                }
                let (w, h) = if *rotated { (part.height, part.width) } else { (part.width, part.height) };
                if rect.width != w || rect.height != h {
                    return Err(TreeError::new(path, "piece_dimension_mismatch",
                        format!("part '{part_id}' expects {w}x{h}, allocated region is {}x{}", rect.width, rect.height)));
                }
                if part.grain_axis.is_some() && rotated_axis(part.grain_axis, *rotated) != self.stock_grain {
                    return Err(TreeError::new(path, "grain_conflict", format!("part '{part_id}' grain orientation differs from board")));
                }
                if w < self.model.machine.min_piece_width || h < self.model.machine.min_piece_height {
                    return Err(TreeError::new(path, "piece_below_machine_minimum", "piece is smaller than machine minimum dimensions"));
                }
                let count = self.counts.entry(part_id.clone()).or_insert(0);
                *count += 1;
                if *count > part.quantity {
                    return Err(TreeError::new(path, "quantity_exceeded", format!("pattern has more copies of '{part_id}' than requested")));
                }
                self.piece_area += rect.area_sq_units();
                self.pieces.push(PlacedPiece { node_id: node_id.clone(), part_id: part_id.clone(), rotated: *rotated, rect });
                Ok(EvaluatedNode::Piece { node_id: node_id.clone(), rect, part_id: part_id.clone(), rotated: *rotated })
            }
            PatternNode::Residual { node_id, reusable } => {
                self.residual_area += rect.area_sq_units();
                self.residuals.push(ResidualRegion { node_id: node_id.clone(), reusable: *reusable, rect });
                Ok(EvaluatedNode::Residual { node_id: node_id.clone(), rect, reusable: *reusable })
            }
        }
    }
}

/// Evaluate exactly ONE pattern instance; does not allocate stock or optimize.
/// Stock rotation changes margins and grain consistently with Stage 3.
/// `validate_model` remains the structural prerequisite; unlike the legacy
/// `validate_model` operation this evaluates actual split geometry.
pub fn evaluate_pattern(model: &CuttingJobModel, pattern_id: &str, board_rotated_90: bool)
    -> Result<EvaluatedPattern, TreeError>
{
    validate_model(model).map_err(|issues| {
        let first = &issues[0];
        TreeError::new(&first.path, "invalid_model", format!("{} [{}]", first.message, first.code))
    })?;
    let pattern = model.patterns.iter().find(|p| p.id == pattern_id)
        .ok_or_else(|| TreeError::new("pattern_id", "unknown_pattern", format!("pattern '{pattern_id}' not found")))?;
    let stock = model.stocks.iter().find(|s| s.id == pattern.stock_id)
        .ok_or_else(|| TreeError::new("pattern.stock_id", "unknown_stock", "referenced stock missing"))?;
    if board_rotated_90 && !model.machine.allow_board_rotation {
        return Err(TreeError::new("board_rotated_90", "board_rotation_forbidden", "machine cannot rotate boards"));
    }
    let (width, height) = if board_rotated_90 { (stock.height, stock.width) } else { (stock.width, stock.height) };
    let whole = Rect::new(0, 0, width, height)
        .map_err(|e| TreeError::new("stock", "invalid_board_rect", e.to_string()))?;
    let usable = whole.inset(rotated_margins(stock.trim, board_rotated_90))
        .map_err(|e| TreeError::new("stock.trim", "invalid_usable_rect", e.to_string()))?;
    let mut context = EvaluationContext {
        model,
        parts: model.parts.iter().map(|p| (p.id.as_str(), p)).collect(),
        stock_grain: rotated_axis(stock.grain_axis, board_rotated_90),
        sequence: direction_sequence(pattern.mode),
        cuts: Vec::new(), pieces: Vec::new(), residuals: Vec::new(),
        counts: BTreeMap::new(), kerf_area: 0, piece_area: 0, residual_area: 0, max_stage: 0,
    };
    let root = context.visit(&pattern.root, usable, None, "root", 0)?;
    let original_area = usable.area_sq_units();
    if original_area != context.kerf_area + context.piece_area + context.residual_area {
        return Err(TreeError::new("root", "area_not_conserved", "tree areas do not sum to usable area"));
    }
    let summary = TreeSummary {
        cuts: context.cuts.len(), placed_pieces: context.pieces.len(),
        residual_regions: context.residuals.len(), stages_used: context.max_stage,
        usable_area_sq_units: original_area.to_string(),
        piece_area_sq_units: context.piece_area.to_string(),
        residual_area_sq_units: context.residual_area.to_string(),
        kerf_area_sq_units: context.kerf_area.to_string(),
        per_part_counts: context.counts, production_validated: false,
    };
    Ok(EvaluatedPattern {
        pattern_id: pattern.id.clone(), stock_id: stock.id.clone(), board_rotated_90,
        mode: pattern.mode, usable_rect: usable, root,
        geometric_cuts: context.cuts, pieces: context.pieces, residuals: context.residuals,
        summary,
    })
}

/// Incremental *structural* builder intended for later packing heuristics.
/// It starts as one residual leaf. Every split replaces one residual with a
/// two-child split; another operation can replace a residual with a piece.
/// Final geometric and stage validity is checked by `evaluate_pattern`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuillotineTreeBuilder {
    root: PatternNode,
}

impl GuillotineTreeBuilder {
    pub fn new(root_id: &str) -> Result<Self, TreeError> {
        Self::valid_id(root_id)?;
        Ok(Self {
            root: PatternNode::Residual { node_id: root_id.to_owned(), reusable: false },
        })
    }

    fn valid_id(value: &str) -> Result<(), TreeError> {
        if value.is_empty() || value.len() > 64 || !value.bytes().all(|b|
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')) {
            return Err(TreeError::new("node_id", "invalid_node_id", "node identifier must be 1..64 ASCII alphanumeric, '.', '_' or '-'"));
        }
        Ok(())
    }

    fn contains(node: &PatternNode, id: &str) -> bool {
        match node {
            PatternNode::Split { node_id, first, second, .. } =>
                node_id == id || Self::contains(first, id) || Self::contains(second, id),
            PatternNode::Piece { node_id, .. } | PatternNode::Residual { node_id, .. } => node_id == id,
        }
    }

    fn target_mut<'a>(node: &'a mut PatternNode, id: &str) -> Option<&'a mut PatternNode> {
        match node {
            PatternNode::Split { first, second, .. } => {
                Self::target_mut(first, id).or_else(|| Self::target_mut(second, id))
            }
            PatternNode::Piece { .. } | PatternNode::Residual { .. } => {
                let found = match &*node {
                    PatternNode::Piece { node_id, .. } | PatternNode::Residual { node_id, .. } => node_id == id,
                    PatternNode::Split { .. } => false,
                };
                if found { Some(node) } else { None }
            }
        }
    }

    pub fn split_residual(&mut self, id: &str, direction: CutDirection, at: Unit,
                          first_id: &str, second_id: &str) -> Result<(), TreeError> {
        for new_id in [first_id, second_id] { Self::valid_id(new_id)?; }
        if first_id == second_id || Self::contains(&self.root, first_id) || Self::contains(&self.root, second_id) {
            return Err(TreeError::new("node_id", "duplicate_node_id", "new child IDs must be unique in the tree"));
        }
        if at == 0 {
            return Err(TreeError::new(id, "invalid_split", "cut offset must be positive"));
        }
        let target = Self::target_mut(&mut self.root, id)
            .ok_or_else(|| TreeError::new(id, "unknown_node", "node does not exist"))?;
        if let PatternNode::Residual { node_id, .. } = target {
            *target = PatternNode::Split {
                node_id: node_id.clone(), direction, at,
                first: Box::new(PatternNode::Residual { node_id: first_id.into(), reusable: false }),
                second: Box::new(PatternNode::Residual { node_id: second_id.into(), reusable: false }),
            };
            Ok(())
        } else {
            Err(TreeError::new(id, "not_residual", "only residual leaves can be split"))
        }
    }

    pub fn place_piece(&mut self, id: &str, part_id: &str, rotated: bool) -> Result<(), TreeError> {
        Self::valid_id(part_id)?;
        let target = Self::target_mut(&mut self.root, id)
            .ok_or_else(|| TreeError::new(id, "unknown_node", "node does not exist"))?;
        if let PatternNode::Residual { node_id, .. } = target {
            *target = PatternNode::Piece { node_id: node_id.clone(), part_id: part_id.into(), rotated };
            Ok(())
        } else {
            Err(TreeError::new(id, "not_residual", "only residual leaves can become pieces"))
        }
    }

    pub fn set_residual_reusable(&mut self, id: &str, reusable: bool) -> Result<(), TreeError> {
        let target = Self::target_mut(&mut self.root, id)
            .ok_or_else(|| TreeError::new(id, "unknown_node", "node does not exist"))?;
        if let PatternNode::Residual { reusable: value, .. } = target {
            *value = reusable;
            Ok(())
        } else {
            Err(TreeError::new(id, "not_residual", "only residual leaves can be marked reusable"))
        }
    }

    pub fn root(&self) -> &PatternNode { &self.root }
    pub fn finish(self) -> PatternNode { self.root }
}

#[cfg(test)]
mod builder_tests {
    use super::*;
    #[test]
    fn builder_constructs_two_stage_tree() {
        let mut b=GuillotineTreeBuilder::new("r").unwrap();
        b.split_residual("r",CutDirection::Vertical,400,"l","right").unwrap();
        b.split_residual("l",CutDirection::Horizontal,300,"a","b").unwrap();
        b.place_piece("a","A",false).unwrap();
        b.set_residual_reusable("b",true).unwrap();
        assert!(matches!(b.root(),PatternNode::Split { .. }));
        let model:CuttingJobModel=serde_json::from_value(
            serde_json::from_str::<serde_json::Value>(include_str!("../../../examples/guillotine-vh.json")).unwrap()["operation"]["model"].clone()
        ).unwrap();
        let mut model=model;
        model.patterns[0].root=b.finish();
        let result=evaluate_pattern(&model,"pattern-01",false).unwrap();
        assert_eq!(result.summary.placed_pieces,1);
        assert_eq!(result.summary.cuts,2);
    }
    #[test]
    fn duplicate_nodes_are_disallowed() {
        let mut b=GuillotineTreeBuilder::new("root").unwrap();
        assert_eq!(b.split_residual("root",CutDirection::Vertical,200,"a","a").unwrap_err().code,"duplicate_node_id");
        assert_eq!(b.split_residual("root",CutDirection::Vertical,200,"root","b").unwrap_err().code,"duplicate_node_id");
    }
    #[test]
    fn splitting_a_piece_is_not_allowed() {
        let mut b=GuillotineTreeBuilder::new("root").unwrap();
        b.place_piece("root","A",false).unwrap();
        assert_eq!(b.split_residual("root",CutDirection::Vertical,200,"l","r").unwrap_err().code,"not_residual");
    }
    #[test]
    fn missing_nodes_are_rejected() {
        let mut b=GuillotineTreeBuilder::new("root").unwrap();
        assert_eq!(b.place_piece("nope","A",false).unwrap_err().code,"unknown_node");
    }
}
