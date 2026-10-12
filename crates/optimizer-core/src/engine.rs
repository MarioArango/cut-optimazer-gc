//! Request boundary, validation, and Stage 16 industrial economics.

use crate::contract::{
    Capabilities, JobOperation, JobOutput, JobRequest, JobResponse, JobStatus,
    ValidationReceipt, SCHEMA_VERSION,
};
use crate::error::EngineError;
use crate::model::validate_model;
use crate::guillotine::evaluate_pattern;
use crate::layout_validation::validate_layout;
use crate::preprocess::preprocess_model;
use crate::two_stage::generate_two_stage;
use crate::three_stage::{generate_three_stage, valid_search_settings, ThreeStageError};
use crate::multi_board::{generate_multi_board, valid_multi_board_settings, MultiBoardError};
use crate::full_board_stacking::plan_full_board_stacks;
use crate::stacked_multi_board::generate_stacked_multi_board;
use crate::production_simulator::simulate_production;
use crate::strip_stacking::plan_strip_stacks;
use crate::dynamic_stacking::plan_dynamic_stacks;
use crate::industrial_scheduler::{schedule_production, validate_schedule_input};
use crate::global_optimizer::{optimize_global, valid_global_options, GlobalError};
use crate::cut_plans::generate_cut_plans;
use crate::economics::{evaluate_economics, compare_economic_layouts, validate_economic_options, EconomicError};

pub fn parse_request(input: &str) -> Result<JobRequest, EngineError> {
    let request: JobRequest =
        serde_json::from_str(input).map_err(|err| EngineError::InvalidJson(err.to_string()))?;
    validate_request(&request)?;
    Ok(request)
}

pub fn validate_request(request: &JobRequest) -> Result<ValidationReceipt, EngineError> {
    if request.schema_version != SCHEMA_VERSION {
        return Err(EngineError::InvalidSchemaVersion {
            received: request.schema_version.clone(),
            supported: SCHEMA_VERSION.to_string(),
        });
    }

    if request.job_id.is_empty()
        || request.job_id.len() > 64
        || !request
            .job_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
    {
        return Err(EngineError::InvalidJobId);
    }

    if !(100..=3_600_000).contains(&request.options.max_runtime_ms) {
        return Err(EngineError::InvalidTimeout {
            received: request.options.max_runtime_ms,
        });
    }

    match &request.operation {
        JobOperation::SystemCheck { echo } => {
            if echo
                .as_ref()
                .is_some_and(|value| value.chars().count() > 120)
            {
                return Err(EngineError::InvalidEchoLength);
            }
        }
        JobOperation::ValidateModel { model } | JobOperation::PreprocessModel { model } => {
            validate_model(model).map_err(EngineError::InvalidModel)?;
        }
        JobOperation::GeometrySplit { rect, direction, at, kerf } => {
            rect.split(*direction, *at, *kerf).map_err(EngineError::Geometry)?;
        }
        JobOperation::GeometryTrim { rect, margins } => {
            rect.inset(*margins).map_err(EngineError::Geometry)?;
        }
        JobOperation::GenerateTwoStage { model, stock_id } => {
            validate_model(model).map_err(EngineError::InvalidModel)?;
            if stock_id.as_ref().is_some_and(|id| !model.stocks.iter().any(|s| &s.id == id)) {
                return Err(EngineError::TwoStage(crate::two_stage::GeneratorError::UnknownStock(
                    stock_id.clone().unwrap_or_default(),
                )));
            }
        }
        JobOperation::GenerateThreeStage { model, stock_id, beam_width, max_expansions } => {
            validate_model(model).map_err(EngineError::InvalidModel)?;
            if !valid_search_settings(*beam_width, *max_expansions) {
                return Err(EngineError::ThreeStage(ThreeStageError::InvalidSearchSettings));
            }
            if stock_id.as_ref().is_some_and(|id| !model.stocks.iter().any(|s| &s.id == id)) {
                return Err(EngineError::ThreeStage(ThreeStageError::UnknownStock(
                    stock_id.clone().unwrap_or_default(),
                )));
            }
        }
        JobOperation::GenerateMultiBoard { model, max_boards, restarts, beam_width, max_expansions } => {
            validate_model(model).map_err(EngineError::InvalidModel)?;
            if !valid_multi_board_settings(*max_boards, *restarts, *beam_width, *max_expansions) {
                return Err(EngineError::MultiBoard(MultiBoardError::InvalidSettings));
            }
        }
        JobOperation::PlanFullBoardStacks { model, layout, costs } => {
            plan_full_board_stacks(model, layout, *costs).map_err(EngineError::FullBoardStack)?;
        }
        JobOperation::GenerateStackedMultiBoard { model, max_boards, restarts, beam_width, max_expansions, .. } => {
            validate_model(model).map_err(EngineError::InvalidModel)?;
            if !valid_multi_board_settings(*max_boards, *restarts, *beam_width, *max_expansions) {
                return Err(EngineError::MultiBoard(MultiBoardError::InvalidSettings));
            }
        }
        JobOperation::SimulateProduction { model, plan } => {
            // Deliberately validate the entire proposed operation trace.
            simulate_production(model, plan).map_err(EngineError::ProductionSimulation)?;
        }
        JobOperation::PlanStripStacks { model, plan, next_cuts, stack_options } => {
            plan_strip_stacks(model, plan, next_cuts, *stack_options).map_err(EngineError::StripStack)?;
        }
        JobOperation::PlanDynamicStacks { model, plan, routes, dynamic_options } => {
            plan_dynamic_stacks(model, plan, routes, *dynamic_options).map_err(EngineError::DynamicStack)?;
        }
        JobOperation::CompareEconomicLayouts { model, layouts, economic_options } => {
            validate_model(model).map_err(EngineError::InvalidModel)?;
            validate_economic_options(economic_options).map_err(EngineError::Economics)?;
            if layouts.is_empty() || layouts.len() > 32 {
                return Err(EngineError::Economics(EconomicError::InvalidSettings(
                    "number of candidate layouts must be 1..32".into()
                )));
            }
            compare_economic_layouts(model, layouts, *economic_options).map_err(EngineError::Economics)?;
        }
        JobOperation::EvaluateEconomics { model, layout, economic_options } => {
            validate_model(model).map_err(EngineError::InvalidModel)?;
            validate_economic_options(economic_options).map_err(EngineError::Economics)?;
            evaluate_economics(model, layout, *economic_options).map_err(EngineError::Economics)?;
        }
        JobOperation::OptimizeGlobal { model, global_options }
        | JobOperation::GenerateCutPlans { model, global_options } => {
            validate_model(model).map_err(EngineError::InvalidModel)?;
            if !valid_global_options(global_options) {
                return Err(EngineError::GlobalOptimization(GlobalError::InvalidSettings));
            }
        }
        JobOperation::ScheduleProduction { model, plan, dependencies, schedule_options } => {
            validate_schedule_input(model, plan, dependencies, *schedule_options,
                request.options.max_runtime_ms).map_err(EngineError::IndustrialScheduler)?;
        }
        JobOperation::ValidateLayout { .. } => {
            // Bad layouts return an informative report; they are well-formed requests.
        }
        JobOperation::GuillotineEvaluate { model, pattern_id, board_rotated_90 } => {
            evaluate_pattern(model, pattern_id, *board_rotated_90).map_err(EngineError::Guillotine)?;
        }
    }

    Ok(ValidationReceipt {
        valid: true,
        schema_version: SCHEMA_VERSION.to_owned(),
        job_id: request.job_id.clone(),
    })
}

pub fn run_request(request: JobRequest) -> Result<JobResponse, EngineError> {
    validate_request(&request)?;

    let output = match request.operation {
        JobOperation::SystemCheck { echo } => JobOutput::SystemCheck {
            core_ready: true,
            optimization_available: true,
            echo,
        },
        JobOperation::ValidateModel { model } => JobOutput::ModelValidation {
            summary: validate_model(&model).map_err(EngineError::InvalidModel)?,
        },
        JobOperation::PreprocessModel { model } => JobOutput::ModelPreprocessing {
            prepared: preprocess_model(&model).map_err(EngineError::Preprocessing)?,
        },
        JobOperation::GeometrySplit { rect, direction, at, kerf } => JobOutput::GeometrySplit {
            split: rect.split(direction, at, kerf).map_err(EngineError::Geometry)?,
        },
        JobOperation::GeometryTrim { rect, margins } => JobOutput::GeometryTrim {
            usable: rect.inset(margins).map_err(EngineError::Geometry)?,
        },
        JobOperation::GenerateTwoStage { model, stock_id } => JobOutput::TwoStageGeneration {
            result: generate_two_stage(&model, stock_id.as_deref(), request.options.seed,
                request.options.max_runtime_ms).map_err(EngineError::TwoStage)?,
        },
        JobOperation::GenerateThreeStage { model, stock_id, beam_width, max_expansions } =>
            JobOutput::ThreeStageGeneration {
                result: generate_three_stage(&model, stock_id.as_deref(), request.options.seed,
                    request.options.max_runtime_ms, beam_width, max_expansions)
                    .map_err(EngineError::ThreeStage)?,
            },
        JobOperation::GenerateMultiBoard { model, max_boards, restarts, beam_width, max_expansions } =>
            JobOutput::MultiBoardGeneration {
                result: generate_multi_board(&model, request.options.seed, request.options.max_runtime_ms,
                    max_boards, restarts, beam_width, max_expansions).map_err(EngineError::MultiBoard)?,
            },
        JobOperation::PlanFullBoardStacks { model, layout, costs } => JobOutput::FullBoardStackPlanning {
            result: plan_full_board_stacks(&model, &layout, costs).map_err(EngineError::FullBoardStack)?,
        },
        JobOperation::GenerateStackedMultiBoard { model, max_boards, restarts, beam_width, max_expansions, costs } =>
            JobOutput::StackedMultiBoardGeneration {
                result: generate_stacked_multi_board(&model, request.options.seed, request.options.max_runtime_ms,
                    max_boards, restarts, beam_width, max_expansions, costs)
                    .map_err(EngineError::StackedMultiBoard)?,
            },
        JobOperation::SimulateProduction { model, plan } => JobOutput::ProductionSimulation {
            result: simulate_production(&model, &plan).map_err(EngineError::ProductionSimulation)?,
        },
        JobOperation::PlanStripStacks { model, plan, next_cuts, stack_options } => JobOutput::StripStackPlanning {
            result: plan_strip_stacks(&model, &plan, &next_cuts, stack_options).map_err(EngineError::StripStack)?,
        },
        JobOperation::PlanDynamicStacks { model, plan, routes, dynamic_options } => JobOutput::DynamicStackPlanning {
            result: plan_dynamic_stacks(&model, &plan, &routes, dynamic_options).map_err(EngineError::DynamicStack)?,
        },
        JobOperation::CompareEconomicLayouts { model, layouts, economic_options } => JobOutput::EconomicComparison {
            result: compare_economic_layouts(&model, &layouts, economic_options).map_err(EngineError::Economics)?,
        },
        JobOperation::EvaluateEconomics { model, layout, economic_options } => JobOutput::EconomicEvaluation {
            result: evaluate_economics(&model, &layout, economic_options).map_err(EngineError::Economics)?,
        },
        JobOperation::OptimizeGlobal { model, global_options } => JobOutput::GlobalOptimization {
            result: optimize_global(&model, request.options.seed, request.options.max_runtime_ms, global_options)
                .map_err(EngineError::GlobalOptimization)?,
        },
        JobOperation::GenerateCutPlans { model, global_options } => JobOutput::CutPlans {
            result: generate_cut_plans(&model, request.options.seed, request.options.max_runtime_ms, global_options)
                .map_err(EngineError::GlobalOptimization)?,
        },
        JobOperation::ScheduleProduction { model, plan, dependencies, schedule_options } =>
            JobOutput::IndustrialScheduling { result: schedule_production(
                &model, &plan, &dependencies, schedule_options, request.options.max_runtime_ms)
                .map_err(EngineError::IndustrialScheduler)? },
        JobOperation::ValidateLayout { model, layout } => JobOutput::LayoutValidation {
            report: validate_layout(&model, &layout),
        },
        JobOperation::GuillotineEvaluate { model, pattern_id, board_rotated_90 } => JobOutput::GuillotineEvaluation {
            evaluated: evaluate_pattern(&model, &pattern_id, board_rotated_90).map_err(EngineError::Guillotine)?,
        },
    };

    Ok(JobResponse {
        schema_version: SCHEMA_VERSION.to_owned(),
        job_id: request.job_id,
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        status: JobStatus::Completed,
        seed: request.options.seed,
        output,
    })
}

pub fn run_json(input: &str) -> Result<String, EngineError> {
    let response = run_request(parse_request(input)?)?;
    serde_json::to_string_pretty(&response)
        .map_err(|err| EngineError::Serialization(err.to_string()))
}

pub fn capabilities() -> Capabilities {
    Capabilities {
        schema_version: SCHEMA_VERSION.to_owned(),
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        supported_operations: vec![
            "system_check".to_owned(),
            "validate_model".to_owned(),
            "preprocess_model".to_owned(),
            "geometry_split".to_owned(),
            "geometry_trim".to_owned(),
            "guillotine_evaluate".to_owned(),
            "validate_layout".to_owned(),
            "generate_two_stage".to_owned(),
            "generate_three_stage".to_owned(),
            "generate_multi_board".to_owned(),
            "plan_full_board_stacks".to_owned(),
            "generate_stacked_multi_board".to_owned(),
            "simulate_production".to_owned(),
            "plan_strip_stacks".to_owned(),
            "plan_dynamic_stacks".to_owned(),
            "schedule_production".to_owned(),
            "optimize_global".to_owned(),
            "evaluate_economics".to_owned(),
            "compare_economic_layouts".to_owned(),
            "generate_cut_plans".to_owned(),
        ],
        optimization_available: true,
        stage: 18,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::ExecutionOptions;

    fn request() -> JobRequest {
        JobRequest {
            schema_version: "1.0".into(),
            job_id: "unit_test-001".into(),
            options: ExecutionOptions::default(),
            operation: JobOperation::SystemCheck {
                echo: Some("ok".to_owned()),
            },
        }
    }

    #[test]
    fn valid_request_is_accepted() {
        let receipt = validate_request(&request()).unwrap();
        assert!(receipt.valid);
    }

    #[test]
    fn wrong_schema_is_rejected() {
        let mut request = request();
        request.schema_version = "2.0".into();
        assert!(matches!(
            validate_request(&request),
            Err(EngineError::InvalidSchemaVersion { .. })
        ));
    }

    #[test]
    fn invalid_job_id_is_rejected() {
        let mut request = request();
        request.job_id = "bad id".into();
        assert_eq!(validate_request(&request), Err(EngineError::InvalidJobId));
    }

    #[test]
    fn invalid_timeout_is_rejected() {
        let mut request = request();
        request.options.max_runtime_ms = 0;
        assert!(matches!(
            validate_request(&request),
            Err(EngineError::InvalidTimeout { .. })
        ));
    }

    #[test]
    fn long_echo_is_rejected() {
        let mut request = request();
        request.operation = JobOperation::SystemCheck {
            echo: Some("é".repeat(121)),
        };
        assert_eq!(validate_request(&request), Err(EngineError::InvalidEchoLength));
    }

    #[test]
    fn deterministic_json_for_equal_request() {
        let json = serde_json::to_string(&request()).unwrap();
        assert_eq!(run_json(&json).unwrap(), run_json(&json).unwrap());
    }
}
