//! Stage 7 integration tests: real two-stage trees, quantity handling,
//! orientation restrictions and evaluation through the independent Stage 5 evaluator.
use optimizer_core::{generate_two_stage, parse_request, run_json, run_request, JobOutput};
use optimizer_core::model::{Axis, StagePattern};
use serde_json::{json, Value};

const FIXTURE: &str = include_str!("../../../examples/generate-two-stage.json");

fn fixture() -> Value { serde_json::from_str(FIXTURE).unwrap() }
fn operate(v: Value) -> optimizer_core::TwoStageResult {
    let req = parse_request(&v.to_string()).unwrap();
    let response = run_request(req).unwrap();
    let JobOutput::TwoStageGeneration { result } = response.output else {
        panic!("expected two-stage generation")
    };
    result
}
fn example_model() -> optimizer_core::model::CuttingJobModel {
    let req = parse_request(FIXTURE).unwrap();
    let optimizer_core::contract::JobOperation::GenerateTwoStage { model, .. } = req.operation else {
        panic!("not a generator request")
    };
    model
}

#[test]
fn generates_genuinely_evaluated_pattern() {
    let result = operate(fixture());
    assert!(result.found);
    assert!(result.complete_order);
    assert!(!result.search_exhausted);
    assert_eq!(result.remaining_part_counts["A"], 0);
    let board = result.board.unwrap();
    assert!(matches!(board.mode, StagePattern::VH | StagePattern::HV));
    assert_eq!(board.evaluated.summary.per_part_counts["A"], 2);
    assert!(board.evaluated.summary.cuts > 0);
    let piece_area: u128 = board.evaluated.summary.piece_area_sq_units.parse().unwrap();
    let residual: u128 = board.evaluated.summary.residual_area_sq_units.parse().unwrap();
    let kerf: u128 = board.evaluated.summary.kerf_area_sq_units.parse().unwrap();
    let whole: u128 = board.evaluated.summary.usable_area_sq_units.parse().unwrap();
    assert_eq!(piece_area + residual + kerf, whole);
}

#[test]
fn returns_pending_quantities_without_pretending_multiboard_completion() {
    let mut v = fixture();
    v["operation"]["model"]["parts"][0]["quantity"] = json!(10);
    let result = operate(v);
    assert!(result.found);
    assert!(!result.complete_order);
    assert!(result.remaining_part_counts["A"] > 0);
    let placed = result.board.unwrap().evaluated.summary.per_part_counts["A"];
    assert_eq!(placed + result.remaining_part_counts["A"], 10);
}

#[test]
fn supports_hv_only_machine() {
    let mut v = fixture();
    v["operation"]["model"]["machine"]["allowed_patterns"] = json!(["H-V"]);
    let result = operate(v);
    assert!(result.found);
    assert_eq!(result.board.unwrap().mode, StagePattern::HV);
}

#[test]
fn will_not_rotate_piece_with_forbidden_grain() {
    let mut v = fixture();
    v["operation"]["model"]["parts"][0]["grain_axis"] = json!("y");
    v["operation"]["model"]["machine"]["allow_board_rotation"] = json!(false);
    let result = operate(v);
    assert!(!result.found);
    assert_eq!(result.remaining_part_counts["A"], 2);
}

#[test]
fn prefers_rotated_part_when_that_is_the_only_fit() {
    let mut v = fixture();
    let m = &mut v["operation"]["model"];
    m["materials"][0]["directional_grain"] = json!(false);
    m["stocks"][0]["grain_axis"] = Value::Null;
    m["stocks"][0]["width"] = json!(300);
    m["stocks"][0]["height"] = json!(500);
    m["stocks"][0]["trim"] = json!({});
    m["parts"][0]["grain_axis"] = Value::Null;
    m["parts"][0]["allow_rotate_90"] = json!(true);
    m["parts"][0]["width"] = json!(500);
    m["parts"][0]["height"] = json!(300);
    m["parts"][0]["quantity"] = json!(1);
    m["machine"]["allow_board_rotation"] = json!(false);
    let result = operate(v);
    assert!(result.found);
    assert!(!result.board.as_ref().unwrap().board_rotated_90);
    assert!(result.board.unwrap().evaluated.pieces[0].rotated);
}

#[test]
fn respects_selected_stock_id() {
    let mut v = fixture();
    v["operation"]["stock_id"] = json!("board1000");
    let result = operate(v);
    assert_eq!(result.board.unwrap().stock_id, "board1000");
}

#[test]
fn rejects_unknown_stock_in_request_boundary() {
    let mut v = fixture();
    v["operation"]["stock_id"] = json!("missing");
    assert!(parse_request(&v.to_string()).is_err());
}

#[test]
fn exact_width_two_strip_layout_has_no_extra_residual() {
    let mut model = example_model();
    model.materials[0].directional_grain = false;
    model.stocks[0].grain_axis = None;
    model.stocks[0].trim = Default::default();
    model.stocks[0].width = 820;
    model.stocks[0].height = 800;
    model.machine.kerf = 20;
    model.machine.allowed_patterns = vec![StagePattern::VH];
    model.parts[0].grain_axis = None;
    model.parts[0].width = 400;
    model.parts[0].height = 800;
    model.parts[0].quantity = 2;
    let result = generate_two_stage(&model, Some("board1000"), 7, 30_000).unwrap();
    assert!(result.complete_order);
    let board = result.board.unwrap();
    assert_eq!(board.evaluated.summary.cuts, 1);
    assert_eq!(board.evaluated.summary.residual_regions, 0);
    assert_eq!(board.evaluated.summary.per_part_counts["A"], 2);
}

#[test]
fn leftover_narrower_than_saw_is_not_misassigned_as_piece() {
    let mut model = example_model();
    model.materials[0].directional_grain = false;
    model.stocks[0].grain_axis = None;
    model.stocks[0].trim = Default::default();
    model.stocks[0].width = 405;
    model.stocks[0].height = 300;
    model.machine.kerf = 10;
    model.parts[0].grain_axis = None;
    model.parts[0].width = 400;
    model.parts[0].height = 300;
    model.parts[0].quantity = 1;
    // The part would fit in a rectangle, but cannot be separated from the
    // 5-unit-wide unwanted side without sawing off material.
    let result = generate_two_stage(&model, None, 1, 30_000).unwrap();
    assert!(!result.found);
}

#[test]
fn repeated_requests_are_deterministic_for_same_seed() {
    assert_eq!(run_json(FIXTURE).unwrap(), run_json(FIXTURE).unwrap());
}

#[test]
fn respects_board_grain_when_rotated() {
    let model = example_model();
    let result = generate_two_stage(&model, None, 1, 30_000).unwrap();
    let board = result.board.unwrap();
    let stock_grain = model.stocks[0].grain_axis;
    let actual = if board.board_rotated_90 {
        stock_grain.map(|a| if a == Axis::X { Axis::Y } else { Axis::X })
    } else { stock_grain };
    for placed in board.evaluated.pieces {
        let grain = model.parts.iter().find(|p| p.id == placed.part_id).unwrap().grain_axis;
        let oriented = if placed.rotated {
            grain.map(|a| if a == Axis::X { Axis::Y } else { Axis::X })
        } else { grain };
        assert_eq!(actual, oriented);
    }
}

#[test]
fn many_small_pieces_do_not_exceed_tree_depth_budget() {
    let mut model = example_model();
    model.materials[0].directional_grain = false;
    model.stocks[0].grain_axis = None;
    model.stocks[0].trim = Default::default();
    model.stocks[0].width = 1000;
    model.stocks[0].height = 50_000;
    model.machine.max_cut_length = 100_000;
    model.parts[0].grain_axis = None;
    model.parts[0].width = 1000;
    model.parts[0].height = 100;
    model.parts[0].quantity = 200;
    let result = generate_two_stage(&model, None, 1, 30_000).unwrap();
    assert!(result.found);
    let board = result.board.unwrap();
    assert!(board.evaluated.pieces.len() <= 96);
}

#[test]
fn full_breadth_strip_is_labelled_with_transposed_pattern() {
    let mut model = example_model();
    model.materials[0].directional_grain = false;
    model.stocks[0].grain_axis = None;
    model.stocks[0].trim = Default::default();
    model.stocks[0].width = 1000;
    model.stocks[0].height = 5_000;
    model.machine.max_cut_length = 100_000;
    model.parts[0].grain_axis = None;
    model.parts[0].width = 1000;
    model.parts[0].height = 100;
    model.parts[0].quantity = 3;
    model.parts.truncate(1);
    let board = generate_two_stage(&model, None, 1, 30_000).unwrap().board.unwrap();
    // No first-stage cut is needed, so the root cut is horizontal: H-V.
    assert_eq!(board.mode, StagePattern::HV);
    assert_eq!(board.pattern.mode, StagePattern::HV);

    model.machine.allowed_patterns = vec![StagePattern::VH];
    model.machine.allow_board_rotation = false;
    let result = generate_two_stage(&model, None, 1, 30_000).unwrap();
    assert!(result.board.is_none_or(|board| board.mode == StagePattern::VH));
}
