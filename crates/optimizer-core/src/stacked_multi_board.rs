//! Stage 10 pipeline: use the existing Stage 9 geometry allocator, then plan
//! conservative complete-board stacks over the independently checked batch.
//! Does not change which boards Stage 9 selects (stack-aware joint search is
//! planned for Stage 15).

use crate::full_board_stacking::{plan_full_board_stacks, FullBoardStackPlan, StackCostModel, StackPlanError};
use crate::model::CuttingJobModel;
use crate::multi_board::{generate_multi_board, MultiBoardError, MultiBoardResult};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackedMultiBoardError {
    Allocation(MultiBoardError),
    Stacking(StackPlanError),
}
impl fmt::Display for StackedMultiBoardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation(e) => write!(f, "multi-board allocation: {e}"),
            Self::Stacking(e) => write!(f, "full-board stacking: {e}"),
        }
    }
}
impl std::error::Error for StackedMultiBoardError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackedMultiBoardResult {
    pub allocation: MultiBoardResult,
    pub full_board_stacking: FullBoardStackPlan,
}

pub fn generate_stacked_multi_board(
    model: &CuttingJobModel, seed: u64, max_runtime_ms: u64,
    max_boards: usize, restarts: usize, beam_width: usize, max_expansions: usize,
    costs: StackCostModel,
) -> Result<StackedMultiBoardResult, StackedMultiBoardError> {
    let allocation = generate_multi_board(model, seed, max_runtime_ms, max_boards,
        restarts, beam_width, max_expansions).map_err(StackedMultiBoardError::Allocation)?;
    let mut referenced_model = model.clone();
    referenced_model.patterns.extend(allocation.boards.iter().map(|b| b.pattern.clone()));
    let full_board_stacking = plan_full_board_stacks(&referenced_model, &allocation.layout, costs)
        .map_err(StackedMultiBoardError::Stacking)?;
    Ok(StackedMultiBoardResult { allocation, full_board_stacking })
}
