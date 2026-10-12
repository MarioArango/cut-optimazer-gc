use optimizer_core::{plan_dynamic_stacks, CutRoute, DecisionKind, DynamicStackOptions,
    DynamicStackError, WorkpieceRoute, ProductionOperation, ProductionPlan};
use optimizer_core::model::{CutDirection, CuttingJobModel};
use serde_json::{json, Value};

fn fixture(name: &str) -> (CuttingJobModel, ProductionPlan, Vec<WorkpieceRoute>, DynamicStackOptions) {
    let s = match name {
        "normal" => include_str!("../../../examples/plan-dynamic-stacks.json"),
        "divergent" => include_str!("../../../examples/plan-dynamic-divergent.json"),
        "repackage" => include_str!("../../../examples/plan-dynamic-repackage.json"),
        "disabled" => include_str!("../../../examples/plan-dynamic-disabled.json"),
        "costly" => include_str!("../../../examples/plan-dynamic-costly.json"),
        "invalid" => include_str!("../../../examples/plan-dynamic-invalid-cut.json"),
        _ => panic!("unknown fixture"),
    };
    let v: Value = serde_json::from_str(s).unwrap();
    let op = &v["operation"];
    (serde_json::from_value(op["model"].clone()).unwrap(),
     serde_json::from_value(op["plan"].clone()).unwrap(),
     serde_json::from_value(op["routes"].clone()).unwrap(),
     serde_json::from_value(op.get("dynamic_options").cloned().unwrap_or(json!({}))).unwrap())
}

fn kinds(result: &optimizer_core::DynamicStackResult) -> Vec<DecisionKind> {
    result.decisions.iter().map(|d| d.kind.clone()).collect()
}

#[test]
fn maintains_three_layers_through_two_shared_cuts() {
    let (model, plan, routes, opts) = fixture("normal");
    let result = plan_dynamic_stacks(&model, &plan, &routes, opts).unwrap();
    assert_eq!(result.standalone_saw_passes, 6);
    assert_eq!(result.planned_saw_passes, 2);
    assert_eq!(result.saw_passes_avoided, 4);
    assert_eq!(result.projected_simulation.summary.saw_passes,
        result.base_simulation.summary.saw_passes + 2);
    assert!(result.projected_simulation.summary.area_conserved);
    assert!(kinds(&result).contains(&DecisionKind::KeepAndCut));
    assert_eq!(kinds(&result).iter().filter(|k| **k == DecisionKind::KeepAndCut).count(), 2);
    assert_eq!(result.terminal_workpiece_ids.len(), 9);
    assert_eq!(result.projected_simulation.summary.active_stacks, 0);
    assert!(result.estimated_net_savings_minor.parse::<i64>().unwrap() > 0);
}

#[test]
fn separates_when_child_routes_diverge_then_cuts_individually() {
    let (model, plan, routes, opts) = fixture("divergent");
    let result = plan_dynamic_stacks(&model, &plan, &routes, opts).unwrap();
    assert_eq!(result.standalone_saw_passes, 4);
    assert_eq!(result.planned_saw_passes, 3);
    assert_eq!(result.saw_passes_avoided, 1);
    let types = kinds(&result);
    assert!(types.contains(&DecisionKind::UnstackDivergent));
    assert_eq!(types.iter().filter(|k| **k == DecisionKind::CutAlone).count(), 2);
    assert!(result.projected_simulation.summary.area_conserved);
}

#[test]
fn releases_old_stack_and_regroups_with_third_free_strip() {
    let (model, plan, routes, opts) = fixture("repackage");
    let result = plan_dynamic_stacks(&model, &plan, &routes, opts).unwrap();
    assert!(kinds(&result).contains(&DecisionKind::UnstackForRegroup));
    assert_eq!(result.planned_saw_passes, 1);
    assert_eq!(result.saw_passes_avoided, 2);
    assert!(result.operations_added.windows(2).any(|w|
        matches!(&w[0], ProductionOperation::Unstack { stack_id } if stack_id == "left")
          && matches!(&w[1], ProductionOperation::Stack { member_ids, .. } if member_ids.len() == 3)));
    assert!(result.projected_simulation.summary.area_conserved);
}

#[test]
fn without_stack_capability_performs_only_single_cuts() {
    let (model, plan, routes, opts) = fixture("disabled");
    let result = plan_dynamic_stacks(&model, &plan, &routes, opts).unwrap();
    assert_eq!(result.standalone_saw_passes, 3);
    assert_eq!(result.planned_saw_passes, 3);
    assert!(!result.operations_added.iter().any(|o| matches!(o, ProductionOperation::Stack { .. })));
    assert!(result.projected_simulation.summary.area_conserved);
}

#[test]
fn refuses_expensive_optional_stacking() {
    let (model, plan, routes, opts) = fixture("costly");
    let result = plan_dynamic_stacks(&model, &plan, &routes, opts).unwrap();
    assert_eq!(result.planned_saw_passes, 3);
    assert_eq!(result.saw_passes_avoided, 0);
}

#[test]
fn invalid_physical_cut_is_rejected_before_producing_a_plan() {
    let (model, plan, routes, opts) = fixture("invalid");
    let err = plan_dynamic_stacks(&model, &plan, &routes, opts).unwrap_err();
    assert!(matches!(err, DynamicStackError::InvalidInput { code, .. } if code == "invalid_cut_geometry"));
}

#[test]
fn rejects_duplicate_unknown_and_consumed_routes() {
    let (model, plan, mut routes, opts) = fixture("normal");
    routes.push(routes[0].clone());
    assert!(matches!(plan_dynamic_stacks(&model, &plan, &routes, opts),
        Err(DynamicStackError::InvalidInput { code, .. }) if code == "duplicate_route"));
    let bad = WorkpieceRoute { workpiece_id: "does_not_exist".into(), route: CutRoute::Done };
    assert!(matches!(plan_dynamic_stacks(&model, &plan, &[bad], opts),
        Err(DynamicStackError::InvalidInput { code, .. }) if code == "unknown_workpiece"));
    let consumed = WorkpieceRoute { workpiece_id: "b0".into(), route: CutRoute::Done };
    assert!(matches!(plan_dynamic_stacks(&model, &plan, &[consumed], opts),
        Err(DynamicStackError::InvalidInput { code, .. }) if code == "ineligible_workpiece"));
}

#[test]
fn max_operations_is_enforced() {
    let (model, plan, routes, mut opts) = fixture("normal");
    opts.max_operations = 1;
    assert!(matches!(plan_dynamic_stacks(&model, &plan, &routes, opts),
        Err(DynamicStackError::InvalidInput { code, .. }) if code == "operation_limit"));
}

#[test]
fn deterministic_and_no_recycled_workpiece_ids() {
    let (model, plan, routes, opts) = fixture("normal");
    let a = plan_dynamic_stacks(&model, &plan, &routes, opts).unwrap();
    let b = plan_dynamic_stacks(&model, &plan, &routes, opts).unwrap();
    assert_eq!(a, b);
    let ids: Vec<_> = a.projected_simulation.workpieces.iter().map(|p| p.id.as_str()).collect();
    let distinct: std::collections::BTreeSet<_> = ids.iter().copied().collect();
    assert_eq!(ids.len(), distinct.len());
}

#[test]
fn exposed_by_versioned_json_request() {
    let result = optimizer_core::run_json(include_str!("../../../examples/plan-dynamic-stacks.json")).unwrap();
    let v: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(v["output"]["type"], "dynamic_stack_planning");
    assert_eq!(v["output"]["result"]["saw_passes_avoided"], 4);
    assert_eq!(v["output"]["result"]["projected_simulation"]["summary"]["production_certified"], false);
}

#[test]
fn cut_route_distinguishes_two_output_branches() {
    let src = json!({"kind":"cut","direction":"vertical","at":200,
        "first":{"kind":"done"},"second":{"kind":"cut","direction":"horizontal","at":100,
            "first":{"kind":"done"},"second":{"kind":"done"}}});
    let route: CutRoute = serde_json::from_value(src.clone()).unwrap();
    assert_eq!(serde_json::to_value(route).unwrap(),src);
    assert_eq!(CutDirection::Vertical, serde_json::from_str::<CutDirection>("\"vertical\"").unwrap());
}

#[test]
fn rejects_children_smaller_than_minimum_machine_dimensions() {
    let (model, plan, mut routes, opts) = fixture("normal");
    routes[0].route = CutRoute::Cut { direction: CutDirection::Vertical,
        at: 5, first: Box::new(CutRoute::Done), second: Box::new(CutRoute::Done) };
    let err = plan_dynamic_stacks(&model, &plan, &routes, opts).unwrap_err();
    assert!(matches!(err, DynamicStackError::InvalidInput { code, .. }
        if code == "minimum_dimensions"));
}

#[test]
fn can_keep_output_stacks_for_later_operations() {
    let (model, plan, routes, mut opts) = fixture("normal");
    opts.release_finished_stacks = false;
    let result = plan_dynamic_stacks(&model, &plan, &routes, opts).unwrap();
    assert_eq!(result.planned_saw_passes, 2);
    assert_eq!(result.projected_simulation.summary.active_stacks, 3);
    assert!(!kinds(&result).contains(&DecisionKind::ReleaseFinished));
}
