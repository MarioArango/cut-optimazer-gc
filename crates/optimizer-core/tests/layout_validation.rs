//! Stage 6 integration: independent whole-layout validation.
use optimizer_core::{capabilities, parse_request, run_json, run_request, JobOutput};
use optimizer_core::layout_validation::{validate_layout, LayoutValidationReport};
use optimizer_core::contract::JobOperation;
use serde_json::{json, Value};

const VALID: &str = include_str!("../../../examples/layout-valid.json");

fn report(json_input: &str) -> LayoutValidationReport {
    let req = parse_request(json_input).unwrap();
    let response = run_request(req).unwrap();
    let JobOutput::LayoutValidation { report } = response.output else { panic!("expected layout validation") };
    report
}
fn altered(edit: impl FnOnce(&mut Value)) -> String {
    let mut data: Value=serde_json::from_str(VALID).unwrap();
    edit(&mut data);
    data.to_string()
}
fn contains(report: &LayoutValidationReport, code: &str) -> bool {
    report.issues.iter().any(|issue| issue.code==code)
}

#[test]
fn capabilities_expose_layout_validation() {
    let caps=capabilities();
    assert_eq!(caps.stage, 18);
    assert!(caps.supported_operations.iter().any(|x| x=="validate_layout"));
    assert!(caps.optimization_available);
}
#[test]
fn valid_two_boards_reconcile_every_part() {
    let result=report(VALID);
    assert!(result.valid,"{:?}",result.issues);
    let s=result.summary;
    assert_eq!(s.boards_geometry_verified,2);
    assert_eq!(s.verified_placed_pieces,2);
    assert_eq!(s.cuts_verified,4);
    assert_eq!(s.per_part_verified.get("A"),Some(&2));
    assert_eq!(s.verified_usable_area_sq_units,"1528800");
    assert_eq!(s.verified_piece_area_sq_units,"240000");
    assert_eq!(s.verified_residual_area_sq_units,"1265200");
    assert_eq!(s.verified_kerf_area_sq_units,"23600");
    assert!(s.count_reconciliation_complete);
    assert!(!s.production_validated);
}
#[test]
fn valid_layout_matches_golden_response() {
    let actual: Value=serde_json::from_str(&run_json(VALID).unwrap()).unwrap();
    let expected: Value=serde_json::from_str(include_str!("../../../examples/expected-layout-validation.json")).unwrap();
    assert_eq!(actual,expected);
}
#[test]
fn missing_piece_is_reported() {
    let r=report(include_str!("../../../examples/layout-invalid-missing.json"));
    assert!(!r.valid);
    assert!(contains(&r,"missing_pieces"));
    assert_eq!(r.summary.missing_per_part.get("A"),Some(&1));
}
#[test]
fn extra_piece_is_reported() {
    let r=report(include_str!("../../../examples/layout-invalid-excess.json"));
    assert!(!r.valid);
    assert!(contains(&r,"excess_pieces"));
    assert_eq!(r.summary.extra_per_part.get("A"),Some(&1));
}
#[test]
fn double_consumption_of_same_physical_board_rejected() {
    let r=report(include_str!("../../../examples/layout-invalid-duplicate-stock.json"));
    assert!(!r.valid);
    assert!(contains(&r,"duplicate_stock_unit"));
    assert_eq!(r.summary.boards_geometry_verified,1);
    assert!(!r.summary.count_reconciliation_complete);
}
#[test]
fn invalid_tree_region_is_reported_without_panic() {
    let r=report(include_str!("../../../examples/layout-invalid-piece-size.json"));
    assert!(!r.valid);
    assert!(contains(&r,"piece_dimension_mismatch"));
    assert_eq!(r.summary.boards_geometry_verified,0);
    assert!(!r.summary.count_reconciliation_complete);
}
#[test]
fn wrong_stage_direction_is_rejected() {
    let r=report(include_str!("../../../examples/layout-invalid-stage.json"));
    assert!(!r.valid);
    assert!(contains(&r,"invalid_stage_transition"));
}
#[test]
fn out_of_range_stock_index_is_rejected() {
    let r=report(include_str!("../../../examples/layout-invalid-inventory.json"));
    assert!(!r.valid);
    assert!(contains(&r,"stock_capacity_exceeded"));
}
#[test]
fn board_rotation_respects_machine_profile() {
    let r=report(include_str!("../../../examples/layout-invalid-rotation.json"));
    assert!(!r.valid);
    assert!(contains(&r,"board_rotation_forbidden"));
}
#[test]
fn part_material_must_match_stock() {
    let r=report(include_str!("../../../examples/layout-invalid-material.json"));
    assert!(!r.valid);
    assert!(contains(&r,"material_mismatch"));
}
#[test]
fn unknown_stock_and_empty_layout_are_distinct_failures() {
    let r=report(include_str!("../../../examples/layout-invalid-stock.json"));
    assert!(contains(&r,"unknown_stock"));
    let r=report(include_str!("../../../examples/layout-invalid-empty.json"));
    assert!(contains(&r,"empty_layout"));
    assert!(contains(&r,"missing_pieces"));
}
#[test]
fn invalid_model_returns_report_not_engine_error() {
    let text=altered(|v|v["operation"]["model"]["parts"][0]["quantity"]=json!(0));
    let r=report(&text);
    assert!(!r.valid);
    assert!(contains(&r,"invalid_quantity"));
    assert_eq!(r.summary.boards_geometry_verified,0);
}
#[test]
fn duplicate_board_id_is_rejected() {
    let text=altered(|v|v["operation"]["layout"]["boards"][1]["board_id"] = json!("board-1"));
    let r=report(&text);
    assert!(!r.valid);
    assert!(contains(&r,"duplicate_board_id"));
}
#[test]
fn invalid_board_id_is_rejected() {
    let text=altered(|v|v["operation"]["layout"]["boards"][0]["board_id"] = json!("bad board id"));
    assert!(contains(&report(&text),"invalid_board_id"));
}
#[test]
fn unsupported_cutting_mode_rejected_by_model_validator() {
    let text=altered(|v|v["operation"]["model"]["machine"]["allowed_patterns"] = json!(["H-V"]));
    let r=report(&text);
    assert!(!r.valid);
    assert!(contains(&r,"unsupported_mode"));
}
#[test]
fn wrong_pattern_stock_id_is_rejected() {
    let text=altered(|v|{
        let stock= v["operation"]["model"]["stocks"][0].clone();
        let mut next=stock; next["id"]=json!("other-stock");
        v["operation"]["model"]["stocks"].as_array_mut().unwrap().push(next);
        v["operation"]["layout"]["boards"][0]["stock_id"]=json!("other-stock");
    });
    let r=report(&text);
    assert!(contains(&r,"stock_pattern_mismatch"));
}
#[test]
fn cut_length_limit_is_checked_independently() {
    let text=altered(|v| v["operation"]["model"]["machine"]["max_cut_length"] = json!(500));
    assert!(contains(&report(&text),"cut_too_long"));
}
#[test]
fn saw_kerf_cannot_exhaust_the_region() {
    let text=altered(|v|v["operation"]["model"]["machine"]["kerf"] = json!(1000));
    assert!(contains(&report(&text),"invalid_split"));
}
#[test]
fn grain_conflict_is_detected() {
    let text=altered(|v|v["operation"]["model"]["parts"][0]["grain_axis"] = json!("y"));
    assert!(contains(&report(&text),"grain_conflict"));
}
#[test]
fn rotated_part_requires_permission() {
    let text=altered(|v|v["operation"]["model"]["patterns"][0]["root"]["first"]["first"]["rotated"] = json!(true));
    assert!(contains(&report(&text),"rotation_forbidden"));
}
#[test]
fn missing_pattern_is_detected() {
    let text=altered(|v|v["operation"]["layout"]["boards"][0]["pattern_id"] = json!("not-found"));
    assert!(contains(&report(&text),"unknown_pattern"));
}
#[test]
fn serialized_json_is_reproducible() {
    assert_eq!(run_json(VALID).unwrap(),run_json(VALID).unwrap());
}
#[test]
fn direct_api_matches_operation() {
    let req=parse_request(VALID).unwrap();
    let JobOperation::ValidateLayout {model, layout}=req.operation else {panic!("wrong operation")};
    assert_eq!(validate_layout(&model,&layout),report(VALID));
}
