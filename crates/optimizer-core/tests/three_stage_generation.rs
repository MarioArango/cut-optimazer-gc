//! Stage 8 contract and algorithm regression tests.
use optimizer_core::{capabilities, generate_three_stage, parse_request, run_json, run_request, JobOutput};
use optimizer_core::model::{CutDirection, StagePattern};
use serde_json::{json, Value};

const VHV: &str = include_str!("../../../examples/generate-three-stage-vhv.json");
const HVH: &str = include_str!("../../../examples/generate-three-stage-hvh.json");

fn fixture() -> Value { serde_json::from_str(VHV).unwrap() }
fn operate(value: Value) -> optimizer_core::ThreeStageResult {
    let response = run_request(parse_request(&value.to_string()).unwrap()).unwrap();
    let JobOutput::ThreeStageGeneration { result } = response.output else {
        panic!("wrong response type")
    };
    result
}
#[test]
fn vhv_places_four_parts_and_reaches_real_third_stage() {
    let result = operate(fixture());
    assert!(result.found);
    assert!(result.complete_order);
    assert!(result.candidates_evaluated > 0);
    let board = result.board.unwrap();
    assert_eq!(board.mode, StagePattern::VHV);
    assert_eq!(board.evaluated.summary.placed_pieces, 4);
    assert_eq!(board.evaluated.summary.stages_used, 3);
    assert!(board.evaluated.geometric_cuts.iter().any(|c| c.stage == 3 && c.direction == CutDirection::Vertical));
    for id in ["A", "B", "C", "D"] { assert_eq!(board.evaluated.summary.per_part_counts[id], 1); }
    let area: u128 = board.evaluated.summary.usable_area_sq_units.parse().unwrap();
    let pieces: u128 = board.evaluated.summary.piece_area_sq_units.parse().unwrap();
    let waste: u128 = board.evaluated.summary.residual_area_sq_units.parse().unwrap();
    let kerf: u128 = board.evaluated.summary.kerf_area_sq_units.parse().unwrap();
    assert_eq!(area, pieces + waste + kerf);
}
#[test]
fn hvh_places_four_parts_and_reaches_third_stage() {
    let request: Value = serde_json::from_str(HVH).unwrap();
    let result = operate(request);
    assert!(result.complete_order);
    let board = result.board.unwrap();
    assert_eq!(board.mode, StagePattern::HVH);
    assert_eq!(board.evaluated.summary.stages_used, 3);
    assert!(board.evaluated.geometric_cuts.iter().any(|c| c.stage == 3 && c.direction == CutDirection::Horizontal));
}
#[test]
fn returns_pending_counts_when_only_one_board_is_generated() {
    let mut request = fixture();
    request["operation"]["model"]["parts"][0]["quantity"] = json!(20);
    let result = operate(request);
    assert!(result.found);
    assert!(!result.complete_order);
    let placed = result.board.unwrap().evaluated.summary.per_part_counts.get("A").copied().unwrap_or(0);
    assert_eq!(placed + result.remaining_part_counts["A"], 20);
}
#[test]
fn rejects_invalid_search_budget() {
    for (beam, expansions) in [(0,100),(65,100),(10,0),(10,20_001)] {
        let mut request=fixture();
        request["operation"]["beam_width"] = json!(beam);
        request["operation"]["max_expansions"] = json!(expansions);
        assert!(parse_request(&request.to_string()).is_err());
    }
}
#[test]
fn no_pieces_fit_returns_an_empty_result_not_success_claim() {
    let v:Value = serde_json::from_str(include_str!("../../../examples/generate-three-stage-no-fit.json")).unwrap();
    let result = operate(v);
    assert!(!result.found);
    assert!(!result.complete_order);
    assert!(result.board.is_none());
}
#[test]
fn board_orientation_restrictions_are_respected() {
    let mut request = fixture();
    request["operation"]["model"]["machine"]["allow_board_rotation"] = json!(false);
    assert!(!operate(request).board.unwrap().board_rotated_90);
}
#[test]
fn allowed_pattern_restrictions_are_respected() {
    let mut request = fixture();
    request["operation"]["model"]["machine"]["allowed_patterns"] = json!(["V-H"]);
    let result = operate(request);
    assert!(!result.found);
}
#[test]
fn can_call_algorithm_as_rust_library() {
    let request = parse_request(VHV).unwrap();
    let optimizer_core::JobOperation::GenerateThreeStage { model, .. } = request.operation else {panic!("operation");};
    let result = generate_three_stage(&model, Some("board1000"), 2026, 30_000, 12, 1500).unwrap();
    assert!(result.found);
}
#[test]
fn seed_is_reproducible_when_budget_is_not_exhausted() {
    let a = run_json(VHV).unwrap();
    let b = run_json(VHV).unwrap();
    assert_eq!(a,b);
}
#[test]
fn unknown_stock_is_rejected_before_search() {
    let mut request = fixture();
    request["operation"]["stock_id"] = json!("does-not-exist");
    assert!(parse_request(&request.to_string()).is_err());
}
#[test]
fn capabilities_include_three_stage_operation() {
    let cap = capabilities();
    assert_eq!(cap.stage, 18);
    assert!(cap.supported_operations.iter().any(|s| s == "generate_three_stage"));
}
#[test]
fn narrow_kerf_incompatible_gap_does_not_create_fake_piece() {
    let mut request = fixture();
    let model = &mut request["operation"]["model"];
    model["parts"] = json!([{
        "id":"X", "material_id":"mel18", "width": 995, "height": 600,
        "quantity":1, "allow_rotate_90":false, "grain_axis":null, "priority":0
    }]);
    let result = operate(request);
    assert!(!result.found);
}
