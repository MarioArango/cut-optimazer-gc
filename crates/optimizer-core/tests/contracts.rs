use optimizer_core::{
    capabilities, parse_request, run_json, run_request, EngineError, JobOperation, JobOutput,
    JobStatus,
};

const DEMO: &str = include_str!("../../../examples/system-check.json");
const MINIMAL: &str = include_str!("../../../examples/system-check-minimal.json");
const EXPECTED: &str = include_str!("../../../examples/expected-system-check.json");

#[test]
fn demo_example_runs() {
    let response = run_request(parse_request(DEMO).unwrap()).unwrap();
    assert_eq!(response.job_id, "demo-stack-001");
    assert_eq!(response.seed, 2026);
    assert_eq!(response.status, JobStatus::Completed);
    assert!(matches!(
        response.output,
        JobOutput::SystemCheck {
            core_ready: true,
            optimization_available: true,
            ..
        }
    ));
}

#[test]
fn minimal_example_applies_defaults() {
    let request = parse_request(MINIMAL).unwrap();
    assert_eq!(request.options.seed, 42);
    assert_eq!(request.options.max_runtime_ms, 30_000);
    assert!(matches!(request.operation, JobOperation::SystemCheck { echo: None }));
}

#[test]
fn output_is_valid_json() {
    let output = run_json(DEMO).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed["output"]["core_ready"], serde_json::Value::Bool(true));
    assert_eq!(parsed["output"]["optimization_available"], serde_json::Value::Bool(true));
}

#[test]
fn unknown_fields_are_not_ignored() {
    let invalid = r#"{"schema_version":"1.0","job_id":"test","operation":{"type":"system_check"},"secret":123}"#;
    assert!(matches!(parse_request(invalid), Err(EngineError::InvalidJson(_))));
}

#[test]
fn unknown_operation_is_rejected() {
    let invalid = r#"{"schema_version":"1.0","job_id":"test","operation":{"type":"optimize"}}"#;
    assert!(matches!(parse_request(invalid), Err(EngineError::InvalidJson(_))));
}

#[test]
fn malformed_json_is_rejected() {
    assert!(matches!(parse_request("{not json}"), Err(EngineError::InvalidJson(_))));
}

#[test]
fn capabilities_are_explicit_about_stage_ten() {
    let info = capabilities();
    assert_eq!(info.stage, 18);
    let required = ["system_check", "validate_model", "preprocess_model", "geometry_split", "geometry_trim", "guillotine_evaluate", "validate_layout", "generate_two_stage", "generate_three_stage", "generate_multi_board", "plan_full_board_stacks", "generate_stacked_multi_board", "simulate_production", "plan_strip_stacks", "plan_dynamic_stacks", "schedule_production", "optimize_global", "evaluate_economics", "compare_economic_layouts"];
    for name in required {
        assert!(capabilities().supported_operations.iter().any(|available| available == name), "missing operation: {name}");
    }
    assert!(info.optimization_available);
}

#[test]
fn demo_result_matches_reference_json() {
    let actual: serde_json::Value = serde_json::from_str(&run_json(DEMO).unwrap()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(EXPECTED).unwrap();
    assert_eq!(actual, expected);
}
