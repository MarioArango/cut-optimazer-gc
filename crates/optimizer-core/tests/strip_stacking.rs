use optimizer_core::{plan_strip_stacks, NextStripCut, ProductionOperation,
    ProductionPlan, StripStackOptions, StripStackError};
use optimizer_core::model::{CutDirection, CuttingJobModel};
use serde_json::{json, Value};

fn fixture() -> (CuttingJobModel, ProductionPlan, Vec<NextStripCut>, StripStackOptions) {
    let value: Value = serde_json::from_str(include_str!("../../../examples/plan-strip-stacks.json")).unwrap();
    let operation = &value["operation"];
    (serde_json::from_value(operation["model"].clone()).unwrap(),
     serde_json::from_value(operation["plan"].clone()).unwrap(),
     serde_json::from_value(operation["next_cuts"].clone()).unwrap(),
     serde_json::from_value(operation["stack_options"].clone()).unwrap())
}

#[test]
fn joins_strips_from_three_different_boards_and_replays_all_cuts() {
    let (model, plan, cuts, opts) = fixture();
    let result = plan_strip_stacks(&model, &plan, &cuts, opts).unwrap();
    assert_eq!(result.groups.len(), 2);
    assert_eq!(result.groups[0].member_ids.len(), 3);
    assert_eq!(result.groups[0].source_board_ids, vec!["b0", "b1", "b2"]);
    assert_eq!(result.groups[1].member_ids.len(), 2);
    assert_eq!(result.saw_passes_avoided, 3);
    assert_eq!(result.operations_added.len(), 4);
    assert!(result.projected_simulation.summary.area_conserved);
    assert_eq!(result.projected_simulation.summary.saw_passes,
        result.base_simulation.summary.saw_passes + 2);
    assert_eq!(result.projected_simulation.summary.active_stacks, 4);
    assert_eq!(result.ungrouped_workpiece_ids.len(), 0);
}

#[test]
fn preserves_already_created_strip_stack_without_restaking() {
    let value: Value = serde_json::from_str(include_str!("../../../examples/plan-strip-stacks-existing.json")).unwrap();
    let op = &value["operation"];
    let result = plan_strip_stacks(
        &serde_json::from_value(op["model"].clone()).unwrap(),
        &serde_json::from_value(op["plan"].clone()).unwrap(),
        &serde_json::from_value::<Vec<NextStripCut>>(op["next_cuts"].clone()).unwrap(),
        StripStackOptions::default()).unwrap();
    assert_eq!(result.groups.len(), 1);
    assert_eq!(result.groups[0].input_stack_id, "left");
    assert_eq!(result.operations_added.len(), 1);
    assert!(matches!(result.operations_added[0], ProductionOperation::CutStack { .. }));
    assert!(result.projected_simulation.summary.area_conserved);
}

#[test]
fn disabling_strip_capability_yields_no_groups() {
    let (mut model, plan, cuts, opts) = fixture();
    model.machine.stacking.strips = false;
    model.machine.stacking.restacking = false;
    let result = plan_strip_stacks(&model, &plan, &cuts, opts).unwrap();
    assert!(result.groups.is_empty());
    assert_eq!(result.ungrouped_workpiece_ids.len(), 5);
}

#[test]
fn no_grouping_when_package_is_too_tall() {
    let (mut model, plan, cuts, opts) = fixture();
    model.machine.max_stack_height = 180;
    let result = plan_strip_stacks(&model, &plan, &cuts, opts).unwrap();
    assert!(result.groups.is_empty());
}

#[test]
fn groups_with_different_next_cuts_do_not_combine() {
    let (model, plan, mut cuts, opts) = fixture();
    cuts[1].at = 320;
    let result = plan_strip_stacks(&model, &plan, &cuts, opts).unwrap();
    assert!(!result.groups.iter().any(|g| g.member_ids.contains(&"b1_left".to_string())
        && g.member_ids.contains(&"b0_left".to_string())));
}

#[test]
fn unprofitable_stacks_rejected_by_default() {
    let (model, plan, cuts, mut opts) = fixture();
    opts.saw_pass_cost_minor = 1;
    opts.stack_handling_cost_minor = 5000;
    let result = plan_strip_stacks(&model, &plan, &cuts, opts).unwrap();
    assert!(result.groups.is_empty());
}

#[test]
fn duplicate_intent_is_rejected() {
    let (model, plan, mut cuts, opts) = fixture();
    cuts.push(cuts[0].clone());
    let err = plan_strip_stacks(&model, &plan, &cuts, opts).unwrap_err();
    assert!(matches!(err, StripStackError::InvalidInput { code, .. } if code == "duplicate_intent"));
}

#[test]
fn unknown_or_consumed_workpiece_is_rejected() {
    let (model, plan, _cuts, opts) = fixture();
    let err = plan_strip_stacks(&model, &plan, &[NextStripCut {workpiece_id: "b0".into(),
        direction:CutDirection::Horizontal,at:300}],opts).unwrap_err();
    assert!(matches!(err, StripStackError::InvalidInput { code, .. } if code == "ineligible_workpiece"));
}

#[test]
fn impossible_next_cut_is_rejected() {
    let (model, plan, mut cuts, opts) = fixture();
    cuts[0].at = 799;
    let err = plan_strip_stacks(&model, &plan, &cuts, opts).unwrap_err();
    assert!(matches!(err, StripStackError::InvalidInput { code, .. } if code == "invalid_next_cut"));
}

#[test]
fn deterministic_for_same_input_and_no_consumed_duplicate() {
    let (model, plan, cuts, opts) = fixture();
    let a = plan_strip_stacks(&model, &plan, &cuts, opts).unwrap();
    let b = plan_strip_stacks(&model, &plan, &cuts, opts).unwrap();
    assert_eq!(a, b);
    let all: Vec<_> = a.groups.iter().flat_map(|g| g.member_ids.iter()).collect();
    let distinct: std::collections::BTreeSet<_> = all.iter().collect();
    assert_eq!(distinct.len(), all.len());
}

#[test]
fn request_is_available_over_json_api() {
    let s = include_str!("../../../examples/plan-strip-stacks.json");
    let result = optimizer_core::run_json(s).unwrap();
    let parsed: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["output"]["type"], json!("strip_stack_planning"));
    assert_eq!(parsed["output"]["result"]["groups"].as_array().unwrap().len(), 2);
}
