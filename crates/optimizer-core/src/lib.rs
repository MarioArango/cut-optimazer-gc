//! Industrial cutting optimizer — Stage 17: independent Rust HTTP API; core remains transport-agnostic.
//!
//! Rectangular operations, saw-kerf splits, insets and coordinate transforms.
//! Stage 14 schedules explicit physical operations; CNC machine certification remains external.

pub mod contract;
pub mod engine;
pub mod error;
pub mod geometry;
pub mod guillotine;
pub mod layout_validation;
pub mod model;
pub mod multi_board;
pub mod preprocess;
pub mod two_stage;
pub mod three_stage;
pub mod deep_stage;
pub mod full_board_stacking;
pub mod stacked_multi_board;
pub mod production_simulator;
pub mod strip_stacking;
pub mod dynamic_stacking;
pub mod industrial_scheduler;
pub mod global_optimizer;
pub mod economics;
pub mod cut_plans;

pub use contract::{
    Capabilities, ExecutionOptions, JobOperation, JobOutput, JobRequest, JobResponse,
    JobStatus, ValidationReceipt, SCHEMA_VERSION,
};
pub use engine::{capabilities, parse_request, run_json, run_request, validate_request};
pub use error::EngineError;
pub use geometry::{GeometryError, Rect, SplitResult};
pub use guillotine::{evaluate_pattern, EvaluatedPattern, EvaluatedNode, EvaluatedCut, PlacedPiece, ResidualRegion, TreeSummary, TreeError, GuillotineTreeBuilder};
pub use model::{validate_model, CuttingJobModel, ModelIssue, ModelSummary};
pub use preprocess::{preprocess_model, PreprocessError, PreprocessedJob, PreprocessSummary};

pub use layout_validation::{validate_layout, BoardAssignment, LayoutPlan, LayoutIssue, LayoutSummary, LayoutValidationReport};

pub use two_stage::{generate_two_stage, GeneratorError, TwoStageBoard, TwoStageHeuristic, TwoStageResult};

pub use deep_stage::{generate_deep_stage, DeepStageBoard, DeepStageError, DeepStageHeuristic, DeepStageResult};

pub use three_stage::{generate_three_stage, ThreeStageError, ThreeStageBoard, ThreeStageHeuristic, ThreeStageResult};

pub use multi_board::{generate_multi_board, MultiBoardError, MultiBoardResult, AllocatedBoard, PatternSource};

pub use full_board_stacking::{plan_full_board_stacks, FullBoardStack, FullBoardStackPlan, StackCostModel, StackPlanError};
pub use stacked_multi_board::{generate_stacked_multi_board, StackedMultiBoardResult, StackedMultiBoardError};

pub use production_simulator::{simulate_production, InitialBoard, ProductionOperation, ProductionPlan,
    PhysicalWorkpiece, PhysicalStack, PhysicalState, ProductionEvent, ProductionSummary,
    ProductionSimulation, SimulationError};

pub use strip_stacking::{plan_strip_stacks, NextStripCut, StripStackOptions, StripStackResult, StripStackGroup, StripStackError};

pub use industrial_scheduler::{schedule_production, validate_schedule_input, ScheduleDependency, ScheduleOptions,
    ScheduledStep, ScheduleResult, SchedulerError};
pub use dynamic_stacking::{plan_dynamic_stacks, CutRoute, WorkpieceRoute, DynamicStackOptions, DynamicStackResult, DynamicStackError, StackDecision, DecisionKind};

pub use global_optimizer::{optimize_global, GlobalSearchOptions, GlobalResult, GlobalError, Improvement, valid_global_options};

pub use economics::{evaluate_economics, compare_economic_layouts, validate_economic_options, EconomicObjective, EconomicRates, RecoveryPolicy, ExtraHandling, EconomicOptions, EconomicReport, EconomicError, EconomicQuote, QuotedMode, SalvageLine, EconomicAreaSummary, EconomicComparison};

pub use cut_plans::{export_cut_plans, generate_cut_plans};
