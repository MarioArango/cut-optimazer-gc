//! Stage 10: conservative full-board stacking with cost-aware grouping.
use optimizer_core::{
    generate_stacked_multi_board, plan_full_board_stacks, parse_request, run_request,
    FullBoardStackPlan, JobOperation, JobOutput, StackCostModel,
};
use optimizer_core::model::{CuttingJobModel, PatternNode};
use optimizer_core::layout_validation::LayoutPlan;
use serde_json::{json, Value};
use std::collections::HashSet;

const SAMPLE: &str = include_str!("../../../examples/plan-full-board-stacks.json");
const COMBINED: &str = include_str!("../../../examples/generate-stacked-multi-board.json");

fn split(v: &str) -> (CuttingJobModel, LayoutPlan, StackCostModel) {
    let request = parse_request(v).expect("valid stack request");
    if let JobOperation::PlanFullBoardStacks { model, layout, costs } = request.operation {
        (model, layout, costs)
    } else { panic!("wrong operation") }
}

fn stack(v: &str) -> FullBoardStackPlan {
    let (model, layout, costs) = split(v);
    plan_full_board_stacks(&model, &layout, costs).expect("valid layout")
}

#[test]
fn identical_boards_are_grouped_by_height_limit() {
    let result = stack(SAMPLE);
    assert_eq!(result.total_boards, 4);
    assert_eq!(result.boards_in_stacks, 3);
    assert_eq!(result.stacks.len(), 1);
    assert_eq!(result.stacks[0].members, 3);
    assert_eq!(result.stacks[0].total_height, 540);
    assert_eq!(result.stacks[0].member_board_ids, vec!["B1", "B2", "B3"]);
    assert_eq!(result.unstacked_board_ids, vec!["B4"]);
    assert_eq!(result.geometric_saw_passes_without_stacking, 8);
    assert_eq!(result.estimated_saw_passes_saved, 4);
    assert_eq!(result.estimated_saw_passes_with_stacking, 4);
    assert_eq!(result.estimated_work_units_without_stacking, "80");
    assert_eq!(result.estimated_work_units_with_stacking, "50");
    assert_eq!(result.estimated_work_units_saved, "30");
    assert!(result.geometry_validated);
    assert!(!result.physical_execution_validated);
}

#[test]
fn no_board_is_duplicated_and_all_physical_units_are_accounted_for() {
    let r = stack(SAMPLE);
    let mut seen = HashSet::new();
    for b in &r.unstacked_board_ids { assert!(seen.insert(b)); }
    for s in &r.stacks { for b in &s.member_board_ids { assert!(seen.insert(b)); } }
    assert_eq!(seen.len(), r.total_boards);
}

#[test]
fn node_ids_and_pattern_ids_do_not_change_geometry_equivalence() {
    let (mut model, mut layout, costs) = split(SAMPLE);
    let mut second = model.patterns[0].clone();
    second.id = "another-pattern-id".into();
    fn rename_tree(tree: &mut PatternNode) {
        match tree {
            PatternNode::Split { node_id, first, second, .. } => {
                *node_id = format!("renamed_{node_id}");
                rename_tree(first); rename_tree(second);
            }
            PatternNode::Piece { node_id, .. } | PatternNode::Residual { node_id, .. } => {
                *node_id = format!("renamed_{node_id}");
            }
        }
    }
    rename_tree(&mut second.root);
    model.patterns.push(second);
    layout.boards[1].pattern_id = "another-pattern-id".into();
    let r = plan_full_board_stacks(&model, &layout, costs).unwrap();
    assert_eq!(r.stacks.len(), 1);
    assert_eq!(r.stacks[0].members, 3);
}

#[test]
fn machine_can_disable_full_board_stacking() {
    let (mut model, layout, costs) = split(SAMPLE);
    model.machine.stacking.full_boards = false;
    let r = plan_full_board_stacks(&model, &layout, costs).unwrap();
    assert!(r.stacks.is_empty());
    assert_eq!(r.unstacked_board_ids.len(), 4);
    assert_eq!(r.estimated_saw_passes_saved, 0);
}

#[test]
fn per_stack_member_limit_is_enforced() {
    let (mut model, layout, costs) = split(SAMPLE);
    model.machine.max_stack_members = 2;
    model.machine.max_stack_height = 800;
    let r = plan_full_board_stacks(&model, &layout, costs).unwrap();
    assert_eq!(r.stacks.len(), 2);
    assert!(r.stacks.iter().all(|s| s.members == 2));
    assert!(r.unstacked_board_ids.is_empty());
}

#[test]
fn setup_and_release_costs_can_make_stacking_unprofitable() {
    let r = stack(include_str!("../../../examples/plan-full-board-stacks-unprofitable.json"));
    assert!(r.stacks.is_empty());
    assert_eq!(r.estimated_work_units_with_stacking, "80");
}

#[test]
fn stock_skus_are_kept_separate_even_with_identical_geometry() {
    let (mut model, mut layout, costs) = split(SAMPLE);
    let mut alt_stock = model.stocks[0].clone();
    alt_stock.id = "another-batch".into();
    model.stocks.push(alt_stock);
    let mut alt_pattern = model.patterns[0].clone();
    alt_pattern.id = "pattern-alt".into();
    alt_pattern.stock_id = "another-batch".into();
    model.patterns.push(alt_pattern);
    for b in &mut layout.boards[2..] {
        b.stock_id = "another-batch".into();
        b.pattern_id = "pattern-alt".into();
        b.stock_unit_index -= 2;
    }
    let r = plan_full_board_stacks(&model, &layout, costs).unwrap();
    assert_eq!(r.stacks.len(), 2);
    assert_eq!(r.stacks.iter().map(|s| s.members).sum::<usize>(), 4);
    assert_ne!(r.stacks[0].stock_id, r.stacks[1].stock_id);
}

#[test]
fn non_identical_pattern_is_not_stacked() {
    let (mut model, mut layout, costs) = split(SAMPLE);
    let mut altered = model.patterns[0].clone();
    altered.id = "changed-residual".into();
    if let PatternNode::Split { second, .. } = &mut altered.root {
        if let PatternNode::Residual { reusable, .. } = second.as_mut() { *reusable = false; }
    }
    model.patterns.push(altered);
    layout.boards[3].pattern_id = "changed-residual".into();
    let r = plan_full_board_stacks(&model, &layout, costs).unwrap();
    assert_eq!(r.stacks.len(), 1);
    assert_eq!(r.stacks[0].members, 3);
    assert_eq!(r.unstacked_board_ids, vec!["B4"]);
}

#[test]
fn duplicate_stock_unit_is_rejected_by_independent_validator() {
    let (model, mut layout, costs) = split(SAMPLE);
    layout.boards[1].stock_unit_index = 0;
    assert!(plan_full_board_stacks(&model, &layout, costs).is_err());
}

#[test]
fn invalid_pattern_geometry_is_rejected() {
    let (mut model, layout, costs) = split(SAMPLE);
    if let PatternNode::Split { at, .. } = &mut model.patterns[0].root { *at = 4000; }
    assert!(plan_full_board_stacks(&model, &layout, costs).is_err());
}

#[test]
fn partial_layout_is_still_stackable_but_never_marked_complete() {
    let (mut model, layout, costs) = split(SAMPLE);
    model.parts[0].quantity = 5;
    let r = plan_full_board_stacks(&model, &layout, costs).unwrap();
    assert_eq!(r.total_boards, 4);
    assert_eq!(r.stacks.len(), 1);
    assert!(!r.physical_execution_validated);
}

#[test]
fn json_operation_produces_same_plan_as_direct_api() {
    let r = run_request(parse_request(SAMPLE).unwrap()).unwrap();
    let JobOutput::FullBoardStackPlanning { result } = r.output else { panic!("unexpected output"); };
    assert_eq!(result, stack(SAMPLE));
}

#[test]
fn combined_stage_nine_and_ten_operation_stays_valid() {
    let r = run_request(parse_request(COMBINED).unwrap()).unwrap();
    let JobOutput::StackedMultiBoardGeneration { result } = r.output else { panic!("unexpected output"); };
    assert!(result.allocation.complete_order, "{:?}", result.allocation.validation.issues);
    assert_eq!(result.allocation.boards.len(), 3);
    assert_eq!(result.full_board_stacking.total_boards, 3);
    assert_eq!(result.full_board_stacking.boards_in_stacks, 3);
    assert_eq!(result.full_board_stacking.stacks.len(), 1);
    assert_eq!(result.full_board_stacking.stacks[0].members, 3);
    assert!(!result.full_board_stacking.physical_execution_validated);
}

#[test]
fn combined_library_api_supports_cost_override() {
    let r = parse_request(COMBINED).unwrap();
    if let JobOperation::GenerateStackedMultiBoard { model, max_boards, restarts, beam_width, max_expansions, costs } = r.operation {
        let generated = generate_stacked_multi_board(&model, 2026, 30000,
            max_boards, restarts, beam_width, max_expansions, costs).unwrap();
        assert!(generated.allocation.complete_order);
    } else { panic!("wrong operation") }
}

#[test]
fn full_board_stacks_are_not_assumed_from_zero_cut_patterns() {
    let mut v: Value = serde_json::from_str(COMBINED).unwrap();
    v["operation"]["model"]["stocks"][0]["height"] = json!(600);
    let r = run_request(parse_request(&v.to_string()).unwrap()).unwrap();
    let JobOutput::StackedMultiBoardGeneration { result } = r.output else { panic!("unexpected output"); };
    assert_eq!(result.allocation.boards.len(), 3);
    assert!(result.full_board_stacking.stacks.is_empty());
}

#[test]
fn remnants_are_not_treated_as_full_boards() {
    let (mut model, layout, costs) = split(SAMPLE);
    model.stocks[0].kind = optimizer_core::model::StockKind::Remnant;
    let r = plan_full_board_stacks(&model, &layout, costs).unwrap();
    assert!(r.stacks.is_empty());
    assert_eq!(r.unstacked_board_ids.len(), 4);
}

#[test]
fn insufficient_height_keeps_boards_individual() {
    let (mut model, layout, costs) = split(SAMPLE);
    model.machine.max_stack_height = 350;
    let r = plan_full_board_stacks(&model, &layout, costs).unwrap();
    assert!(r.stacks.is_empty());
    assert_eq!(r.unstacked_board_ids.len(), 4);
}

#[test]
fn zero_cut_cost_disables_stacking_without_fabricated_savings() {
    let (model, layout, mut costs) = split(SAMPLE);
    costs.cut_cost_units = 0;
    let r = plan_full_board_stacks(&model, &layout, costs).unwrap();
    assert!(r.stacks.is_empty());
    assert_eq!(r.estimated_work_units_saved, "0");
}
