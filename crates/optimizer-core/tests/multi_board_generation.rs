//! Stage 9: allocation, inventory exhaustion, mixed materials and JSON API.
use optimizer_core::{capabilities, generate_multi_board, parse_request, run_json, run_request, JobOutput};
use optimizer_core::model::CuttingJobModel;
use serde_json::{json, Value};
use std::collections::HashSet;

const COMPLETE: &str = include_str!("../../../examples/generate-multi-board.json");
const PARTIAL: &str = include_str!("../../../examples/generate-multi-board-partial.json");
const LIMIT: &str = include_str!("../../../examples/generate-multi-board-limit.json");
const MIXED: &str = include_str!("../../../examples/generate-multi-board-mixed.json");
const THREE: &str = include_str!("../../../examples/generate-multi-board-three-stage.json");

fn operate(input: &str) -> optimizer_core::MultiBoardResult {
    let request = parse_request(input).expect("request is valid");
    let response = run_request(request).expect("generator succeeds");
    match response.output {
        JobOutput::MultiBoardGeneration { result } => result,
        _ => panic!("expected multi-board output"),
    }
}

#[test]
fn three_identical_panels_require_three_physical_boards() {
    let r = operate(COMPLETE);
    assert!(r.complete_order, "{:#?}", r.validation.issues);
    assert!(r.validation.valid);
    assert_eq!(r.boards.len(), 3);
    assert_eq!(r.remaining_part_counts["PANEL"], 0);
    assert_eq!(r.validation.summary.verified_placed_pieces, 3);
    assert_eq!(r.placed_piece_area_sq_units, "1800000");
    let mut used = HashSet::new();
    let mut ids = HashSet::new();
    for b in &r.boards {
        assert!(ids.insert(b.board_id.clone()));
        assert!(used.insert((b.stock_id.clone(), b.stock_unit_index)));
        assert_eq!(b.pattern.id, b.evaluated.pattern_id);
        assert_eq!(b.pattern.stock_id, b.stock_id);
        assert!(b.stock_unit_index < 2);
        assert_eq!(b.evaluated.summary.placed_pieces, 1);
        assert!(matches!(b.mode, optimizer_core::model::StagePattern::VH | optimizer_core::model::StagePattern::HV));
    }
    assert_eq!(r.layout.boards.len(), r.boards.len());
}

#[test]
fn ranking_can_use_cheaper_inventory_and_reports_real_cost() {
    let r = operate(COMPLETE);
    assert_eq!(r.stock_usage["cheap"], 2);
    assert_eq!(r.stock_usage["expensive"], 1);
    assert_eq!(r.stock_cost_minor, "10500");
}

#[test]
fn insufficient_stock_returns_valid_partial_allocation_not_false_success() {
    let r = operate(PARTIAL);
    assert!(!r.complete_order);
    assert!(!r.validation.valid); // missing pieces are a legitimate partial report
    assert_eq!(r.boards.len(), 3);
    assert_eq!(r.remaining_part_counts["PANEL"], 2);
    assert_eq!(r.validation.summary.boards_geometry_verified, 3);
    assert!(r.validation.issues.iter().all(|i| i.code == "missing_pieces"));
    assert_eq!(r.stock_usage["cheap"], 2);
    assert_eq!(r.stock_usage["expensive"], 1);
}

#[test]
fn board_limit_is_respected_and_result_marked_exhausted() {
    let r = operate(LIMIT);
    assert!(!r.complete_order);
    assert!(r.search_exhausted);
    assert_eq!(r.boards.len(), 2);
    assert_eq!(r.remaining_part_counts["PANEL"], 1);
}

#[test]
fn mixed_materials_do_not_share_sheets() {
    let r = operate(MIXED);
    assert!(r.complete_order);
    assert_eq!(r.boards.len(), 5);
    assert_eq!(r.stock_usage["mdf-stock"], 2);
    let req = parse_request(MIXED).unwrap();
    let optimizer_core::JobOperation::GenerateMultiBoard { model, .. } = req.operation else { panic!("operation") };
    for b in &r.boards {
        let s = model.stocks.iter().find(|s| s.id == b.stock_id).unwrap();
        for p in &b.evaluated.pieces {
            let part = model.parts.iter().find(|part| part.id == p.part_id).unwrap();
            assert_eq!(s.material_id, part.material_id);
        }
    }
}

#[test]
fn v_h_v_only_machine_can_generate_multi_board_layout() {
    let r = operate(THREE);
    assert!(!r.boards.is_empty());
    assert!(r.boards.iter().all(|b| b.mode == optimizer_core::model::StagePattern::VHV));
    assert!(r.validation.summary.boards_geometry_verified == r.boards.len());
}

#[test]
fn no_part_can_fit_is_reported_without_invented_infeasibility_proof() {
    let mut v: Value = serde_json::from_str(COMPLETE).unwrap();
    v["operation"]["model"]["parts"][0]["width"] = json!(1100);
    let r = operate(&v.to_string());
    assert!(!r.complete_order);
    assert!(r.boards.is_empty());
    assert_eq!(r.remaining_part_counts["PANEL"], 3);
    assert!(!r.validation.valid);
    assert!(r.note.contains("Partial"));
}

#[test]
fn invalid_limits_rejected_at_json_boundary() {
    for (field, value) in [("max_boards",0),("max_boards",513),("restarts",0),("restarts",9),("beam_width",0),("beam_width",65),("max_expansions",0)] {
        let mut v: Value = serde_json::from_str(COMPLETE).unwrap();
        v["operation"][field] = json!(value);
        assert!(parse_request(&v.to_string()).is_err(), "{field}={value}");
    }
}

#[test]
fn json_entrypoint_is_reproducible_with_generous_time_budget() {
    assert_eq!(run_json(COMPLETE).unwrap(), run_json(COMPLETE).unwrap());
}

#[test]
fn library_api_matches_public_contract() {
    let request = parse_request(COMPLETE).unwrap();
    let optimizer_core::JobOperation::GenerateMultiBoard { model, max_boards, restarts, beam_width, max_expansions } = request.operation else { panic!("operation") };
    let r = generate_multi_board(&model, 2026, 30000, max_boards, restarts, beam_width, max_expansions).unwrap();
    assert!(r.complete_order);
}

#[test]
fn capabilites_report_stage_nine() {
    let info = capabilities();
    assert_eq!(info.stage, 18);
    assert!(info.supported_operations.contains(&"generate_multi_board".to_string()));
}

#[test]
fn original_input_model_is_not_mutated() {
    let request = parse_request(COMPLETE).unwrap();
    let optimizer_core::JobOperation::GenerateMultiBoard { model, .. } = request.operation else { panic!("operation") };
    let copy: CuttingJobModel = model.clone();
    generate_multi_board(&model, 2026, 30000, 10, 2, 8, 250).unwrap();
    assert_eq!(model, copy);
}
