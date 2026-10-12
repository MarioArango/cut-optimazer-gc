//! Request, serialization and typed model validation errors.

use std::fmt;
use crate::geometry::GeometryError;
use crate::guillotine::TreeError;
use crate::model::ModelIssue;
use crate::preprocess::PreprocessError;
use crate::two_stage::GeneratorError;
use crate::three_stage::ThreeStageError;
use crate::multi_board::MultiBoardError;
use crate::full_board_stacking::StackPlanError;
use crate::stacked_multi_board::StackedMultiBoardError;
use crate::production_simulator::SimulationError;
use crate::strip_stacking::StripStackError;
use crate::dynamic_stacking::DynamicStackError;
use crate::industrial_scheduler::SchedulerError;
use crate::global_optimizer::GlobalError;
use crate::economics::EconomicError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    InvalidJson(String),
    InvalidSchemaVersion { received: String, supported: String },
    InvalidJobId,
    InvalidTimeout { received: u64 },
    InvalidEchoLength,
    InvalidModel(Vec<ModelIssue>),
    Preprocessing(PreprocessError),
    Geometry(GeometryError),
    Guillotine(TreeError),
    TwoStage(GeneratorError),
    ThreeStage(ThreeStageError),
    MultiBoard(MultiBoardError),
    FullBoardStack(StackPlanError),
    StackedMultiBoard(StackedMultiBoardError),
    ProductionSimulation(SimulationError),
    StripStack(StripStackError),
    DynamicStack(DynamicStackError),
    IndustrialScheduler(SchedulerError),
    GlobalOptimization(GlobalError),
    Economics(EconomicError),
    Serialization(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(message) => write!(f, "invalid request JSON: {message}"),
            Self::InvalidSchemaVersion { received, supported } => {
                write!(f, "unsupported schema version '{received}' (supported: '{supported}')")
            }
            Self::InvalidJobId => write!(
                f,
                "job_id must contain 1..64 ASCII letters, digits, dots, underscores or hyphens"
            ),
            Self::InvalidTimeout { received } => write!(
                f,
                "max_runtime_ms must be between 100 and 3,600,000 (received: {received})"
            ),
            Self::InvalidEchoLength => write!(f, "echo must contain at most 120 characters"),
            Self::InvalidModel(issues) => {
                write!(f, "invalid cutting model ({} issue(s))", issues.len())?;
                for issue in issues.iter().take(10) {
                    write!(f, "\n  {} [{}]: {}", issue.path, issue.code, issue.message)?;
                }
                if issues.len() > 10 {
                    write!(f, "\n  ... {} more", issues.len() - 10)?;
                }
                Ok(())
            },
            Self::Preprocessing(error) => write!(f, "{error}"),
            Self::Geometry(error) => write!(f, "invalid geometry: {error}"),
            Self::Guillotine(error) => write!(f, "invalid guillotine tree: {error}"),
            Self::TwoStage(error) => write!(f, "two-stage generator: {error}"),
            Self::ThreeStage(error) => write!(f, "three-stage generator: {error}"),
            Self::MultiBoard(error) => write!(f, "multi-board generator: {error}"),
            Self::FullBoardStack(error) => write!(f, "full-board stacking: {error}"),
            Self::StackedMultiBoard(error) => write!(f, "stack-aware multi-board generator: {error}"),
            Self::ProductionSimulation(error) => write!(f, "production simulation: {error}"),
            Self::StripStack(error) => write!(f, "strip stack optimizer: {error}"),
            Self::DynamicStack(error) => write!(f, "dynamic stack optimizer: {error}"),
            Self::IndustrialScheduler(error) => write!(f, "industrial scheduler: {error}"),
            Self::GlobalOptimization(error) => write!(f, "global optimizer: {error}"),
            Self::Economics(error) => write!(f, "economic evaluation: {error}"),
            Self::Serialization(message) => write!(f, "failed to serialize response: {message}"),
        }
    }
}

impl std::error::Error for EngineError {}
