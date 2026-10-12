//! Stage 4 public API: exact geometry operations, not board-layout optimization.
use optimizer_core::{capabilities, parse_request, run_json, run_request, EngineError, GeometryError, JobOutput, Rect};
use optimizer_core::model::{CutDirection, TrimMargins};
use serde_json::Value;

const SPLIT: &str = include_str!("../../../examples/geometry-vertical-split.json");
const EXPECTED_SPLIT: &str = include_str!("../../../examples/expected-geometry-vertical-split.json");
const TRIM: &str = include_str!("../../../examples/geometry-trim.json");
const EXPECTED_TRIM: &str = include_str!("../../../examples/expected-geometry-trim.json");
const INVALID: &str = include_str!("../../../examples/geometry-invalid-cut.json");

#[test]
fn vertical_cut_matches_reference() {
    let actual: Value = serde_json::from_str(&run_json(SPLIT).unwrap()).unwrap();
    let expected: Value = serde_json::from_str(EXPECTED_SPLIT).unwrap();
    assert_eq!(actual, expected);
}
#[test]
fn trims_match_reference() {
    let actual: Value = serde_json::from_str(&run_json(TRIM).unwrap()).unwrap();
    let expected: Value = serde_json::from_str(EXPECTED_TRIM).unwrap();
    assert_eq!(actual, expected);
}
#[test]
fn geometry_is_advertised_without_optimization() {
    let caps = capabilities();
    assert_eq!(caps.stage, 18);
    let required = ["system_check", "validate_model", "preprocess_model", "geometry_split", "geometry_trim", "guillotine_evaluate", "validate_layout", "generate_two_stage", "generate_three_stage", "generate_multi_board", "plan_full_board_stacks", "generate_stacked_multi_board", "simulate_production", "plan_strip_stacks", "plan_dynamic_stacks", "schedule_production", "optimize_global", "evaluate_economics", "compare_economic_layouts"];
    for name in required {
        assert!(capabilities().supported_operations.iter().any(|available| available == name), "missing operation: {name}");
    }
    assert!(caps.optimization_available);
}
#[test]
fn invalid_split_is_rejected_in_validation() {
    assert!(matches!(parse_request(INVALID), Err(EngineError::Geometry(GeometryError::InvalidSplit))));
}
#[test]
fn response_has_split_kind() {
    let result = run_request(parse_request(SPLIT).unwrap()).unwrap();
    let JobOutput::GeometrySplit { split } = result.output else { panic!("missing split output") };
    assert_eq!(split.first.width, 400);
    assert_eq!(split.second.width, 568);
}
#[test]
fn horizontal_split_works_through_public_json_api() {
    let json = include_str!("../../../examples/geometry-horizontal-split.json");
    let value: Value = serde_json::from_str(&run_json(json).unwrap()).unwrap();
    assert_eq!(value["output"]["split"]["first"]["height"], 200);
    assert_eq!(value["output"]["split"]["saw_path"]["y"], 280);
    assert_eq!(value["output"]["split"]["second"]["y"], 310);
}
#[test]
fn split_with_u32_coordinate_overflow_is_rejected() {
    let mut value: Value = serde_json::from_str(SPLIT).unwrap();
    value["operation"]["rect"]["x"] = serde_json::json!(4294967295u64);
    let text = serde_json::to_string(&value).unwrap();
    assert!(matches!(parse_request(&text), Err(EngineError::Geometry(GeometryError::CoordinateOverflow))));
}
#[test]
fn negative_coordinates_are_rejected_by_json() {
    let mut value: Value = serde_json::from_str(SPLIT).unwrap();
    value["operation"]["rect"]["x"] = serde_json::json!(-10);
    assert!(matches!(parse_request(&value.to_string()), Err(EngineError::InvalidJson(_))));
}
#[test]
fn zero_width_is_rejected_even_if_struct_deserialized() {
    let mut value: Value = serde_json::from_str(TRIM).unwrap();
    value["operation"]["rect"]["width"] = serde_json::json!(0);
    assert!(matches!(parse_request(&value.to_string()), Err(EngineError::Geometry(GeometryError::EmptyRectangle))));
}
#[test]
fn geometry_round_trip_preserves_contract() {
    let req = parse_request(SPLIT).unwrap();
    let text = serde_json::to_string(&req).unwrap();
    assert_eq!(req, parse_request(&text).unwrap());
}
#[test]
fn manually_constructed_rectangles_support_repeated_splits() {
    let root = Rect::new(0,0,24400,18300).unwrap();
    let a = root.split(CutDirection::Vertical,8000,32).unwrap();
    let b = a.second.split(CutDirection::Horizontal,9000,32).unwrap();
    assert_eq!(a.first.width,8000);
    assert_eq!(b.first.height,9000);
    assert_eq!(b.second.height,9268);
    assert_eq!(root.area_sq_units(), a.first.area_sq_units()+a.saw_path.area_sq_units()+a.second.area_sq_units());
}
#[test]
fn margins_do_not_automatically_consume_saw_kerf() {
    let root = Rect::new(0,0,24400,18300).unwrap();
    let usable = root.inset(TrimMargins{top:100,bottom:100,left:100,right:100}).unwrap();
    assert_eq!(usable.width,24200);
    assert_eq!(usable.height,18100);
}
