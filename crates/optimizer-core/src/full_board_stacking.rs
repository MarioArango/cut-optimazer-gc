//! Stage 10: conservative stacking of COMPLETE, geometrically identical boards.
//!
//! Uses the independently validated Stage 6 layout, Stage 5 pattern evaluator,
//! and the machine's full-board stack limits. This is an opportunity/operation
//! estimate, NOT a certified production program. Strips, partial cut-prefix
//! sharing, clamp reachability and machine scheduling belong to later stages.

use crate::guillotine::evaluate_pattern;
use crate::layout_validation::{validate_layout, LayoutPlan};
use crate::model::{CutDirection, CuttingJobModel, PatternNode, StagePattern, StockKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackCostModel {
    /// Abstract work units per geometric saw line on an individual board.
    /// Equal for stacks and single boards; calibrated machine times later.
    pub cut_cost_units: u64,
    /// Abstract work units to assemble a stack; per stack of >=2 boards.
    pub stack_setup_cost_units: u64,
    /// Abstract work units to separate one completed stack.
    pub stack_release_cost_units: u64,
}

impl Default for StackCostModel {
    fn default() -> Self {
        Self {
            cut_cost_units: 10,
            stack_setup_cost_units: 5,
            stack_release_cost_units: 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackPlanError {
    InvalidModel(Vec<String>),
    InvalidLayout(Vec<String>),
    Internal(String),
}

impl fmt::Display for StackPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModel(errors) => write!(f, "invalid model: {}", errors.join("; ")),
            Self::InvalidLayout(errors) => write!(f, "invalid layout: {}", errors.join("; ")),
            Self::Internal(msg) => write!(f, "stack planner internal error: {msg}"),
        }
    }
}
impl std::error::Error for StackPlanError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FullBoardStack {
    pub stack_id: String,
    /// Physical stock units, in deterministic bottom-to-top order.
    pub member_board_ids: Vec<String>,
    pub member_stock_unit_indices: Vec<u32>,
    pub stock_id: String,
    pub representative_pattern_id: String,
    pub mode: StagePattern,
    pub board_rotated_90: bool,
    pub members: usize,
    /// Machine stack height in tenths of a millimeter.
    pub total_height: u64,
    /// Cut lines in ONE common pattern (not production operations).
    pub common_geometric_cuts: usize,
    pub estimated_saw_passes_saved: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FullBoardStackPlan {
    pub stacks: Vec<FullBoardStack>,
    /// Includes boards with no matching pattern, remnants, and uneconomic stacks.
    pub unstacked_board_ids: Vec<String>,
    pub total_boards: usize,
    pub boards_in_stacks: usize,
    pub geometric_saw_passes_without_stacking: u64,
    pub estimated_saw_passes_with_stacking: u64,
    pub estimated_saw_passes_saved: u64,
    /// Abstract units, NOT seconds or actual machine running time.
    pub estimated_work_units_without_stacking: String,
    pub estimated_work_units_with_stacking: String,
    pub estimated_work_units_saved: String,
    pub geometry_validated: bool,
    pub physical_execution_validated: bool,
    pub note: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct RepeatKey {
    stock_id: String,
    board_rotated_90: bool,
    mode: u8,
    shape: Vec<u8>,
}

#[derive(Clone, Debug)]
struct EligibleBoard {
    board_id: String,
    stock_unit_index: u32,
    stock_id: String,
    pattern_id: String,
    mode: StagePattern,
    rotated: bool,
    thickness: u64,
    cuts: u64,
}

fn mode_tag(mode: StagePattern) -> u8 {
    // Position in StagePattern::ALL: 0..=3 for the original 2/3-level patterns.
    StagePattern::ALL.iter().position(|&m| m == mode).expect("every pattern is listed") as u8
}

fn u32_bytes(bytes: &mut Vec<u8>, value: u32) { bytes.extend_from_slice(&value.to_le_bytes()); }

fn string_bytes(bytes: &mut Vec<u8>, text: &str) {
    // Length-delimited to ensure unambiguous canonical keys even for non-ASCII part ids.
    let length = u32::try_from(text.len()).unwrap_or(u32::MAX);
    u32_bytes(bytes, length);
    bytes.extend_from_slice(text.as_bytes());
}

fn node_shape(node: &PatternNode, out: &mut Vec<u8>) {
    match node {
        PatternNode::Split { direction, at, first, second, .. } => {
            out.push(b'S');
            out.push(match direction { CutDirection::Vertical => b'V', CutDirection::Horizontal => b'H' });
            u32_bytes(out, *at);
            node_shape(first, out);
            node_shape(second, out);
        }
        PatternNode::Piece { part_id, rotated, .. } => {
            out.push(b'P');
            string_bytes(out, part_id);
            out.push(u8::from(*rotated));
        }
        PatternNode::Residual { reusable, .. } => {
            out.push(b'R');
            out.push(u8::from(*reusable));
        }
    }
}

/// Estimate full-board stacks from a previously generated or supplied layout.
///
/// Only identical full-board layouts from the same Stock SKU, orientation,
/// mode and canonical cut tree may stack. Node IDs and pattern IDs are ignored.
/// Every physical board is used at most once. A bundle is selected only when
/// the reduced saw-pass proxy strictly offsets setup and release costs.
///
/// This deliberately does not group different patterns by common initial cuts.
/// That requires the physical simulator and sequence planner (stages 11–14).
pub fn plan_full_board_stacks(
    model: &CuttingJobModel,
    layout: &LayoutPlan,
    costs: StackCostModel,
) -> Result<FullBoardStackPlan, StackPlanError> {
    crate::model::validate_model(model).map_err(|errors| StackPlanError::InvalidModel(
        errors.into_iter().map(|e| format!("{}:{}", e.path, e.code)).collect()))?;
    let report = validate_layout(model, layout);
    // Partial stage-9 layouts are allowed: all their placed boards still need
    // valid geometry and stock ownership. Only missing-piece and empty-layout
    // diagnoses are expected for partial layouts.
    let unexpected: Vec<String> = report.issues.iter()
        .filter(|e| e.code != "missing_pieces" && e.code != "empty_layout")
        .map(|e| format!("{}:{}", e.path, e.code)).collect();
    if !unexpected.is_empty() || report.summary.boards_geometry_verified != layout.boards.len() {
        return Err(StackPlanError::InvalidLayout(unexpected));
    }
    let stock_map: BTreeMap<&str, _> = model.stocks.iter().map(|s| (s.id.as_str(), s)).collect();
    let pattern_map: BTreeMap<&str, _> = model.patterns.iter().map(|p| (p.id.as_str(), p)).collect();
    let mut groups: BTreeMap<RepeatKey, Vec<EligibleBoard>> = BTreeMap::new();
    let mut single_ids: BTreeSet<String> = BTreeSet::new();
    let mut total_passes: u64 = 0;
    for board in &layout.boards {
        let stock = stock_map.get(board.stock_id.as_str()).ok_or_else(||
            StackPlanError::Internal(format!("missing stock {}", board.stock_id)))?;
        let pattern = pattern_map.get(board.pattern_id.as_str()).ok_or_else(||
            StackPlanError::Internal(format!("missing pattern {}", board.pattern_id)))?;
        let evaluated = evaluate_pattern(model, &board.pattern_id, board.board_rotated_90)
            .map_err(|e| StackPlanError::InvalidLayout(vec![e.to_string()]))?;
        let cuts = u64::try_from(evaluated.summary.cuts).unwrap_or(u64::MAX);
        total_passes = total_passes.saturating_add(cuts);
        let cap_by_height = if stock.thickness > 0 {
            u64::from(model.machine.max_stack_height) / u64::from(stock.thickness)
        } else { 0 };
        let can_stack = model.machine.stacking.full_boards
            && stock.kind == StockKind::FullBoard
            && cap_by_height >= 2
            && model.machine.max_stack_members >= 2
            && cuts > 0;
        if !can_stack {
            single_ids.insert(board.board_id.clone());
            continue;
        }
        let mut shape = Vec::new();
        node_shape(&pattern.root, &mut shape);
        groups.entry(RepeatKey {
            stock_id: board.stock_id.clone(),
            board_rotated_90: board.board_rotated_90,
            mode: mode_tag(pattern.mode),
            shape,
        }).or_default().push(EligibleBoard {
            board_id: board.board_id.clone(),
            stock_unit_index: board.stock_unit_index,
            stock_id: board.stock_id.clone(),
            pattern_id: board.pattern_id.clone(),
            mode: pattern.mode,
            rotated: board.board_rotated_90,
            thickness: u64::from(stock.thickness),
            cuts,
        });
    }
    let mut stacks: Vec<FullBoardStack> = Vec::new();
    let handling = u128::from(costs.stack_setup_cost_units)
        + u128::from(costs.stack_release_cost_units);
    let mut saved_passes = 0_u64;
    for (_, mut members) in groups {
        members.sort_by(|a, b| a.stock_unit_index.cmp(&b.stock_unit_index)
            .then(a.board_id.cmp(&b.board_id)));
        let Some(first) = members.first() else { continue; };
        let limit_height = usize::try_from(u64::from(model.machine.max_stack_height) / first.thickness)
            .unwrap_or(usize::MAX);
        let group_cap = usize::try_from(model.machine.max_stack_members).unwrap_or(usize::MAX)
            .min(limit_height).max(1);
        for chunk in members.chunks(group_cap) {
            if chunk.len() < 2 {
                for board in chunk { single_ids.insert(board.board_id.clone()); }
                continue;
            }
            let candidate_saved = u64::try_from(chunk.len() - 1).unwrap_or(u64::MAX)
                .saturating_mul(first.cuts);
            let gross = u128::from(candidate_saved) * u128::from(costs.cut_cost_units);
            if gross <= handling {
                for board in chunk { single_ids.insert(board.board_id.clone()); }
                continue;
            }
            saved_passes = saved_passes.saturating_add(candidate_saved);
            let serial = stacks.len() + 1;
            stacks.push(FullBoardStack {
                stack_id: format!("full_stack_{serial:05}"),
                member_board_ids: chunk.iter().map(|b| b.board_id.clone()).collect(),
                member_stock_unit_indices: chunk.iter().map(|b| b.stock_unit_index).collect(),
                stock_id: first.stock_id.clone(),
                representative_pattern_id: first.pattern_id.clone(),
                mode: first.mode,
                board_rotated_90: first.rotated,
                members: chunk.len(),
                total_height: first.thickness * chunk.len() as u64,
                common_geometric_cuts: usize::try_from(first.cuts).unwrap_or(usize::MAX),
                estimated_saw_passes_saved: candidate_saved,
            });
        }
    }
    let with_passes = total_passes.saturating_sub(saved_passes);
    let without = u128::from(total_passes) * u128::from(costs.cut_cost_units);
    let with = u128::from(with_passes) * u128::from(costs.cut_cost_units)
        + u128::try_from(stacks.len()).unwrap_or(u128::MAX) * handling;
    let savings = without.saturating_sub(with);
    let stacked_boards: usize = stacks.iter().map(|s| s.members).sum();
    if stacked_boards + single_ids.len() != layout.boards.len() {
        return Err(StackPlanError::Internal("some physical boards were duplicated or lost".into()));
    }
    Ok(FullBoardStackPlan {
        stacks,
        unstacked_board_ids: single_ids.into_iter().collect(),
        total_boards: layout.boards.len(),
        boards_in_stacks: stacked_boards,
        geometric_saw_passes_without_stacking: total_passes,
        estimated_saw_passes_with_stacking: with_passes,
        estimated_saw_passes_saved: saved_passes,
        estimated_work_units_without_stacking: without.to_string(),
        estimated_work_units_with_stacking: with.to_string(),
        estimated_work_units_saved: savings.to_string(),
        geometry_validated: unexpected.is_empty(),
        physical_execution_validated: false,
        note: "Only complete identical full-board cut trees from the same stock SKU and board orientation are grouped. Saw passes and work units are ESTIMATES, not a certified machine schedule. Strip stacking and dynamic splitting are deferred.".into(),
    })
}
