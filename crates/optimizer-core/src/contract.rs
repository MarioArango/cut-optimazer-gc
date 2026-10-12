//! Versioned contracts for API, CLI, and future TypeScript integration.
//! Stage 16 adds independent industrial cost and remnant evaluation.

use serde::{Deserialize, Serialize};
use crate::model::{CutDirection, CuttingJobModel, ModelSummary, TrimMargins, Unit};
use crate::geometry::{Rect, SplitResult};
use crate::guillotine::EvaluatedPattern;
use crate::layout_validation::{LayoutPlan, LayoutValidationReport};
use crate::preprocess::PreprocessedJob;
use crate::two_stage::TwoStageResult;
use crate::three_stage::{ThreeStageResult, default_beam_width, default_max_expansions};
use crate::multi_board::{MultiBoardResult, default_max_boards, default_restarts};
use crate::full_board_stacking::{FullBoardStackPlan, StackCostModel};
use crate::stacked_multi_board::StackedMultiBoardResult;
use crate::production_simulator::{ProductionPlan, ProductionSimulation};
use crate::strip_stacking::{NextStripCut, StripStackOptions, StripStackResult};
use crate::dynamic_stacking::{WorkpieceRoute, DynamicStackOptions, DynamicStackResult};
use crate::industrial_scheduler::{ScheduleDependency, ScheduleOptions, ScheduleResult};
use crate::global_optimizer::{GlobalSearchOptions, GlobalResult};
use crate::economics::{EconomicOptions, EconomicReport, EconomicComparison};

pub const SCHEMA_VERSION: &str = "1.0";

fn default_seed() -> u64 {
    42
}

fn default_timeout_ms() -> u64 {
    30_000
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionOptions {
    /// Reproducibility seed for future heuristics; not used by system_check.
    #[serde(default = "default_seed")]
    pub seed: u64,
    /// Budget for future optimization; no long-running work in Stage 1.
    #[serde(default = "default_timeout_ms")]
    pub max_runtime_ms: u64,
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            seed: default_seed(),
            max_runtime_ms: default_timeout_ms(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum JobOperation {
    SystemCheck {
        #[serde(default)]
        echo: Option<String>,
    },
    ValidateModel {
        model: CuttingJobModel,
    },
    PreprocessModel {
        model: CuttingJobModel,
    },
    /// Standalone geometry demonstration. Does not assert a complete layout.
    GeometrySplit {
        rect: Rect,
        direction: CutDirection,
        at: Unit,
        kerf: Unit,
    },
    GeometryTrim {
        rect: Rect,
        margins: TrimMargins,
    },
    /// Validate and geometrically evaluate one supplied pattern (not a solver).
    ValidateLayout {
        model: CuttingJobModel,
        layout: LayoutPlan,
    },
    /// Generate one automatically constructed guillotine board (V-H or H-V).
    GenerateTwoStage {
        model: CuttingJobModel,
        #[serde(default)]
        stock_id: Option<String>,
    },
    GenerateThreeStage {
        model: CuttingJobModel,
        #[serde(default)]
        stock_id: Option<String>,
        #[serde(default = "default_beam_width")]
        beam_width: usize,
        #[serde(default = "default_max_expansions")]
        max_expansions: usize,
    },
    GenerateMultiBoard {
        model: CuttingJobModel,
        #[serde(default = "default_max_boards")]
        max_boards: usize,
        #[serde(default = "default_restarts")]
        restarts: usize,
        #[serde(default = "default_beam_width")]
        beam_width: usize,
        #[serde(default = "default_max_expansions")]
        max_expansions: usize,
    },
    /// Estimate cost-aware full-board stacking over an existing validated layout.
    PlanFullBoardStacks {
        model: CuttingJobModel,
        layout: LayoutPlan,
        #[serde(default)]
        costs: StackCostModel,
    },
    /// Generate Stage 9 geometry then group repeatable full-board patterns.
    GenerateStackedMultiBoard {
        model: CuttingJobModel,
        #[serde(default = "default_max_boards")]
        max_boards: usize,
        #[serde(default = "default_restarts")]
        restarts: usize,
        #[serde(default = "default_beam_width")]
        beam_width: usize,
        #[serde(default = "default_max_expansions")]
        max_expansions: usize,
        #[serde(default)]
        costs: StackCostModel,
    },
    /// Execute a supplied physical sequence, with lineage and area conservation.
    SimulateProduction {
        model: CuttingJobModel,
        plan: ProductionPlan,
    },
    /// Automatically group compatible cut strips to share their immediate next cut.
    PlanStripStacks {
        model: CuttingJobModel,
        plan: ProductionPlan,
        next_cuts: Vec<NextStripCut>,
        #[serde(default)]
        stack_options: StripStackOptions,
    },
    /// Dynamically share cuts, separate divergent packages and regroup strips.
    PlanDynamicStacks {
        model: CuttingJobModel,
        plan: ProductionPlan,
        routes: Vec<WorkpieceRoute>,
        #[serde(default)]
        dynamic_options: DynamicStackOptions,
    },
    CompareEconomicLayouts {
        model: CuttingJobModel,
        layouts: Vec<LayoutPlan>,
        #[serde(default)]
        economic_options: EconomicOptions,
    },
    EvaluateEconomics {
        model: CuttingJobModel,
        layout: LayoutPlan,
        #[serde(default)]
        economic_options: EconomicOptions,
    },
    OptimizeGlobal {
        model: CuttingJobModel,
        #[serde(default)]
        global_options: GlobalSearchOptions,
    },
    /// Optimise globally and return ready-to-use cutting plans (`cut_plans`).
    GenerateCutPlans {
        model: CuttingJobModel,
        #[serde(default = "crate::cut_plans::default_cut_plan_search")]
        global_options: GlobalSearchOptions,
    },
    ScheduleProduction {
        model: CuttingJobModel,
        plan: ProductionPlan,
        #[serde(default)]
        dependencies: Vec<ScheduleDependency>,
        #[serde(default)]
        schedule_options: ScheduleOptions,
    },
    GuillotineEvaluate {
        model: CuttingJobModel,
        pattern_id: String,
        #[serde(default)]
        board_rotated_90: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobRequest {
    pub schema_version: String,
    pub job_id: String,
    #[serde(default)]
    pub options: ExecutionOptions,
    pub operation: JobOperation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Completed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobOutput {
    SystemCheck {
        core_ready: bool,
        optimization_available: bool,
        echo: Option<String>,
    },
    ModelValidation {
        summary: ModelSummary,
    },
    ModelPreprocessing {
        prepared: PreprocessedJob,
    },
    GeometrySplit {
        split: SplitResult,
    },
    GeometryTrim {
        usable: Rect,
    },
    LayoutValidation {
        report: LayoutValidationReport,
    },
    TwoStageGeneration { result: TwoStageResult },
    ThreeStageGeneration { result: ThreeStageResult },
    MultiBoardGeneration { result: MultiBoardResult },
    FullBoardStackPlanning { result: FullBoardStackPlan },
    StackedMultiBoardGeneration { result: StackedMultiBoardResult },
    ProductionSimulation { result: ProductionSimulation },
    StripStackPlanning { result: StripStackResult },
    DynamicStackPlanning { result: DynamicStackResult },
    IndustrialScheduling { result: ScheduleResult },
    GlobalOptimization { result: GlobalResult },
    CutPlans { result: serde_json::Value },
    EconomicEvaluation { result: EconomicReport },
    EconomicComparison { result: EconomicComparison },
    GuillotineEvaluation {
        evaluated: EvaluatedPattern,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobResponse {
    pub schema_version: String,
    pub job_id: String,
    pub engine_version: String,
    pub status: JobStatus,
    pub seed: u64,
    pub output: JobOutput,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationReceipt {
    pub valid: bool,
    pub schema_version: String,
    pub job_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub schema_version: String,
    pub engine_version: String,
    pub supported_operations: Vec<String>,
    pub optimization_available: bool,
    pub stage: u8,
}
