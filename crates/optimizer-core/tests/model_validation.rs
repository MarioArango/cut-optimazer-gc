//! Stage 2 structural contract regression tests. No geometry or optimization.
use optimizer_core::{capabilities, parse_request, run_json, validate_model, EngineError, JobOperation, JobOutput};
use serde_json::{json, Value};

const EXAMPLE: &str = include_str!("../../../examples/validate-model.json");
const EXPECTED: &str = include_str!("../../../examples/expected-model-validation.json");

fn input() -> Value {
    serde_json::from_str(EXAMPLE).unwrap()
}

fn validate_value(value: Value) -> Result<optimizer_core::model::ModelSummary, Vec<optimizer_core::ModelIssue>> {
    let model: optimizer_core::CuttingJobModel = serde_json::from_value(value["operation"]["model"].clone()).unwrap();
    validate_model(&model)
}

fn issue(value: Value, code: &str) {
    let result = validate_value(value).expect_err("invalid model must be rejected");
    assert!(result.iter().any(|err| err.code == code), "expected {code}; got {result:?}");
}

#[test]
fn complete_model_validates() {
    let req = parse_request(EXAMPLE).unwrap();
    assert_eq!(req.job_id, "modelo-melamina-001");
    let JobOperation::ValidateModel { model } = req.operation else { panic!("expected validate_model") };
    let summary = validate_model(&model).unwrap();
    assert_eq!(summary.requested_pieces, 12);
    assert_eq!(summary.available_boards, 10);
    assert_eq!(summary.pattern_templates, 1);
    assert_eq!(summary.stacked_workpieces, 2);
    assert!(!summary.geometry_validated && !summary.production_validated);
}

#[test]
fn demo_matches_reference() {
    let actual: Value = serde_json::from_str(&run_json(EXAMPLE).unwrap()).unwrap();
    let expected: Value = serde_json::from_str(EXPECTED).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn second_operation_is_advertised() {
    let caps = capabilities();
    assert_eq!(caps.stage, 18);
    let required = ["system_check", "validate_model", "preprocess_model", "geometry_split", "geometry_trim", "guillotine_evaluate", "validate_layout", "generate_two_stage", "generate_three_stage", "generate_multi_board", "plan_full_board_stacks", "generate_stacked_multi_board", "simulate_production", "plan_strip_stacks", "plan_dynamic_stacks", "schedule_production", "optimize_global", "evaluate_economics", "compare_economic_layouts"];
    for name in required {
        assert!(capabilities().supported_operations.iter().any(|available| available == name), "missing operation: {name}");
    }
    assert!(caps.optimization_available);
}

#[test]
fn job_output_is_model_validation() {
    let result = optimizer_core::run_request(parse_request(EXAMPLE).unwrap()).unwrap();
    assert!(matches!(result.output, JobOutput::ModelValidation { .. }));
}

#[test]
fn model_round_trips_without_losing_information() {
    let request = parse_request(EXAMPLE).unwrap();
    let serialized = serde_json::to_string(&request).unwrap();
    assert_eq!(parse_request(&serialized).unwrap(), request);
}

#[test]
fn invalid_reference_is_rejected() {
    let mut v = input();
    v["operation"]["model"]["parts"][0]["material_id"] = json!("missing-material");
    issue(v, "unknown_reference");
}

#[test]
fn duplicate_part_ids_are_rejected() {
    let mut v = input();
    v["operation"]["model"]["parts"][1]["id"] = json!("p-puerta");
    issue(v, "duplicate_id");
}

#[test]
fn zero_dimensions_are_rejected() {
    let mut v = input();
    v["operation"]["model"]["parts"][0]["width"] = json!(0);
    issue(v, "dimension_out_of_range");
}

#[test]
fn zero_quantities_are_rejected() {
    let mut v = input();
    v["operation"]["model"]["stocks"][0]["quantity"] = json!(0);
    issue(v, "invalid_quantity");
}

#[test]
fn invalid_trim_is_rejected() {
    let mut v = input();
    v["operation"]["model"]["stocks"][0]["trim"]["left"] = json!(24400);
    issue(v, "invalid_trim");
}

#[test]
fn inconsistent_thickness_is_rejected() {
    let mut v = input();
    v["operation"]["model"]["stocks"][0]["thickness"] = json!(160);
    issue(v, "thickness_mismatch");
}

#[test]
fn missing_stock_grain_is_rejected() {
    let mut v = input();
    v["operation"]["model"]["stocks"][0]["grain_axis"] = Value::Null;
    issue(v, "grain_mismatch");
}

#[test]
fn part_without_grain_is_allowed_on_directional_material() {
    let mut v = input();
    v["operation"]["model"]["parts"][1]["grain_axis"] = Value::Null;
    let model: optimizer_core::model::CuttingJobModel =
        serde_json::from_value(v["operation"]["model"].clone()).unwrap();
    assert!(optimizer_core::validate_model(&model).is_ok());
}

#[test]
fn duplicate_allowed_patterns_are_rejected() {
    let mut v = input();
    v["operation"]["model"]["machine"]["allowed_patterns"] = json!(["V-H", "V-H"]);
    issue(v, "duplicate_mode");
}

#[test]
fn restacking_requires_strip_capability() {
    let mut v = input();
    v["operation"]["model"]["machine"]["stacking"]["strips"] = json!(false);
    issue(v, "invalid_capability");
}

#[test]
fn exceeds_stack_height_is_rejected() {
    let mut v = input();
    v["operation"]["model"]["machine"]["max_stack_height"] = json!(300);
    issue(v, "stack_height_exceeded");
}

#[test]
fn duplicate_stack_member_is_rejected() {
    let mut v = input();
    v["operation"]["model"]["stacks"][0]["member_ids"] = json!(["b-001", "b-001"]);
    issue(v, "duplicate_stack_member");
}

#[test]
fn bad_stack_registration_is_rejected() {
    let mut v = input();
    v["operation"]["model"]["stacks"][0]["registration_edge"] = json!("right");
    issue(v, "registration_mismatch");
}

#[test]
fn unknown_stack_member_is_rejected() {
    let mut v = input();
    v["operation"]["model"]["stacks"][0]["member_ids"][1] = json!("not-created");
    issue(v, "unknown_reference");
}

#[test]
fn same_stock_board_id_cannot_appear_twice() {
    let mut v = input();
    v["operation"]["model"]["workpieces"][1]["origin_board_key"] = json!("tablero-001");
    issue(v, "duplicate_origin");
}

#[test]
fn missing_parent_for_strip_is_rejected() {
    let mut v = input();
    v["operation"]["model"]["workpieces"][1]["kind"] = json!("strip");
    issue(v, "missing_parent");
}

#[test]
fn cyclic_parent_links_are_rejected() {
    let mut v = input();
    v["operation"]["model"]["stacks"] = json!([]);
    for i in 0..2 {
        v["operation"]["model"]["workpieces"][i]["kind"] = json!("strip");
        v["operation"]["model"]["workpieces"][i]["origin_board_key"] = json!("tablero-001");
    }
    v["operation"]["model"]["workpieces"][0]["parent_workpiece_id"] = json!("b-002");
    v["operation"]["model"]["workpieces"][1]["parent_workpiece_id"] = json!("b-001");
    issue(v, "parent_cycle");
}

#[test]
fn duplicated_pattern_node_ids_are_rejected() {
    let mut v = input();
    v["operation"]["model"]["patterns"][0]["root"]["second"]["node_id"] = json!("n-pieza");
    issue(v, "duplicate_id");
}

#[test]
fn unknown_part_in_pattern_is_rejected() {
    let mut v = input();
    v["operation"]["model"]["patterns"][0]["root"]["first"]["part_id"] = json!("missing");
    issue(v, "unknown_reference");
}

#[test]
fn unsupported_machine_pattern_is_rejected() {
    let mut v = input();
    v["operation"]["model"]["machine"]["allowed_patterns"] = json!(["H-V"]);
    issue(v, "unsupported_mode");
}

#[test]
fn unrecognized_model_field_is_rejected_at_parse_boundary() {
    let mut v = input();
    v["operation"]["model"]["parts"][0]["unrecognized"] = json!(1);
    let json = serde_json::to_string(&v).unwrap();
    assert!(matches!(parse_request(&json), Err(EngineError::InvalidJson(_))));
}

#[test]
fn wrong_orientation_value_is_rejected_at_parse_boundary() {
    let mut v = input();
    v["operation"]["model"]["workpieces"][0]["orientation"] = json!(180);
    assert!(matches!(parse_request(&v.to_string()), Err(EngineError::InvalidJson(_))));
}

#[test]
fn invalid_model_is_returned_by_public_request_api() {
    let example = include_str!("../../../examples/invalid-model.json");
    assert!(matches!(parse_request(example), Err(EngineError::InvalidModel(_))));
}

#[test]
fn valid_model_without_patterns_or_physical_state() {
    let mut v = input();
    v["operation"]["model"]["patterns"] = json!([]);
    v["operation"]["model"]["workpieces"] = json!([]);
    v["operation"]["model"]["stacks"] = json!([]);
    let summary = validate_value(v).unwrap();
    assert_eq!(summary.pattern_templates, 0);
    assert_eq!(summary.stacks, 0);
}

#[test]
fn structurally_valid_does_not_claim_cut_geometry_is_valid() {
    let mut v = input();
    v["operation"]["model"]["patterns"][0]["root"]["at"] = json!(1_000_000);
    let summary = validate_value(v).unwrap();
    assert!(!summary.geometry_validated);
}

#[test]
fn strip_stack_structurally_valid_with_two_parents() {
    let source = include_str!("../../../examples/validate-strip-stack.json");
    let request = parse_request(source).unwrap();
    let JobOperation::ValidateModel { model } = request.operation else { panic!("expected model") };
    let summary = validate_model(&model).unwrap();
    assert_eq!(summary.tracked_workpieces, 4);
    assert_eq!(summary.stacks, 1);
    assert_eq!(summary.stacked_workpieces, 2);
    assert!(!summary.production_validated);
}

#[test]
fn stock_inventory_cannot_be_overstated_by_physical_origins() {
    let mut v = input();
    v["operation"]["model"]["stocks"][0]["quantity"] = json!(1);
    issue(v, "stock_capacity_exceeded");
}

#[test]
fn pending_node_must_exist_in_pattern_tree() {
    let mut v = input();
    v["operation"]["model"]["workpieces"][0]["pending_node_id"] = json!("not-in-tree");
    issue(v, "unknown_reference");
}

#[test]
fn full_board_stack_must_be_enabled() {
    let mut v = input();
    v["operation"]["model"]["machine"]["stacking"]["full_boards"] = json!(false);
    issue(v, "stacking_disabled");
}
