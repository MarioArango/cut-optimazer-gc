//! Stage 15: non-regression, inventory ownership, partial handling, stacking.
use optimizer_core::{optimize_global, GlobalSearchOptions, JobOutput, parse_request, run_request, capabilities};
use optimizer_core::model::CuttingJobModel;
use serde_json::{json,Value};
use std::collections::HashSet;

const FULL: &str = include_str!("../../../examples/optimize-global.json");
const PARTIAL: &str = include_str!("../../../examples/optimize-global-partial.json");
const NO_STACK: &str = include_str!("../../../examples/optimize-global-no-stacking.json");
const INVALID: &str = include_str!("../../../examples/optimize-global-invalid-options.json");

fn run(json: &str) -> optimizer_core::GlobalResult {
    let request = parse_request(json).expect("valid input");
    let response = run_request(request).expect("successful global search");
    match response.output { JobOutput::GlobalOptimization { result } => result,
        other => panic!("unexpected output: {other:?}") }
}

#[test]
fn completed_order_is_never_worsened_and_uses_unique_stock_units() {
    let r = run(FULL);
    assert!(r.initial.complete_order);
    assert!(r.best.complete_order);
    assert!(r.best.validation.valid);
    assert!(r.best_objective_minor.parse::<u128>().unwrap() <= r.initial_objective_minor.parse::<u128>().unwrap());
    assert!(r.best.boards.len() <= 10);
    let stock: HashSet<_> = r.best.boards.iter().map(|b| (b.stock_id.clone(), b.stock_unit_index)).collect();
    let board_ids: HashSet<_> = r.best.boards.iter().map(|b| b.board_id.clone()).collect();
    assert_eq!(stock.len(), r.best.boards.len());
    assert_eq!(board_ids.len(), r.best.boards.len());
    assert_eq!(r.best.layout.boards.len(), r.best.boards.len());
    assert_eq!(r.best.remaining_part_counts["PANEL"], 0);
    assert!(r.iterations_completed <= 3);
    assert!(r.neighbors_validated <= r.iterations_completed);
}

#[test]
fn insufficient_inventory_does_not_fake_completion() {
    let r = run(PARTIAL);
    assert!(!r.best.complete_order);
    assert!(!r.best.validation.valid);
    assert!(r.best.remaining_part_counts["PANEL"] > 0);
    assert_eq!(r.best.validation.summary.boards_geometry_verified, r.best.boards.len());
    assert!(r.best.validation.issues.iter().all(|i| i.code == "missing_pieces" || i.code == "empty_layout"));
}

#[test]
fn disabled_stacking_produces_no_full_board_packages() {
    let r = run(NO_STACK);
    assert!(r.best_stacks.stacks.is_empty());
    assert_eq!(r.best_stacks.estimated_saw_passes_saved, 0);
}

#[test]
fn rejects_wrong_limits_at_public_boundary() {
    assert!(parse_request(INVALID).is_err());
    for (field, val) in [("neighborhood_boards",0),("neighborhood_boards",9),
                         ("max_iterations",65),("operation_weight_minor",1000001)] {
        let mut v:Value = serde_json::from_str(FULL).unwrap();
        v["operation"]["global_options"][field] = json!(val);
        assert!(parse_request(&v.to_string()).is_err(), "{field}={val}");
    }
}

#[test]
fn input_is_not_mutated() {
    let req = parse_request(FULL).unwrap();
    let optimizer_core::JobOperation::OptimizeGlobal { model, global_options } = req.operation else {panic!("op")};
    let copy: CuttingJobModel = model.clone();
    let r = optimize_global(&model, 2026, 4000, global_options).unwrap();
    assert_eq!(model, copy);
    assert_eq!(r.best.validation.summary.boards_geometry_verified, r.best.boards.len());
}

#[test]
fn stage_15_capabilities() {
    let cap = capabilities();
    assert_eq!(cap.stage,18);
    assert!(cap.supported_operations.contains(&"optimize_global".to_owned()));
}

#[test]
fn global_settings_default_is_valid() {
    assert!(optimizer_core::valid_global_options(&GlobalSearchOptions::default()));
}
