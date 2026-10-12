use optimizer_core::{schedule_production, ScheduleDependency, ScheduleOptions,
    SchedulerError, ProductionOperation, ProductionPlan};
use optimizer_core::model::CuttingJobModel;
use serde_json::{json, Value};

fn fixture(name: &str) -> (CuttingJobModel, ProductionPlan, Vec<ScheduleDependency>, ScheduleOptions) {
    let text = match name {
        "normal" => include_str!("../../../examples/schedule-production.json"),
        "stages" => include_str!("../../../examples/schedule-production-invalid-stages.json"),
        "cycle" => include_str!("../../../examples/schedule-production-cycle.json"),
        "terminal" => include_str!("../../../examples/schedule-production-terminal.json"),
        _ => panic!("unknown fixture"),
    };
    let v: Value = serde_json::from_str(text).unwrap();
    let o = &v["operation"];
    (serde_json::from_value(o["model"].clone()).unwrap(),
     serde_json::from_value(o["plan"].clone()).unwrap(),
     serde_json::from_value(o.get("dependencies").cloned().unwrap_or(json!([]))).unwrap(),
     serde_json::from_value(o.get("schedule_options").cloned().unwrap_or(json!({}))).unwrap())
}

#[test]
fn reorder_unordered_operations_and_replay_validly() {
    let (model, plan, deps, opts) = fixture("normal");
    let result = schedule_production(&model, &plan, &deps, opts, 30_000).unwrap();
    assert_eq!(result.scheduled_plan.operations.len(), 8);
    assert_ne!(result.ordered_source_indices, (0..8).collect::<Vec<_>>());
    assert!(result.simulation.summary.area_conserved);
    assert_eq!(result.cut_passes, 2);
    assert_eq!(result.stack_operations, 1);
    assert_eq!(result.unstack_operations, 1);
    assert_eq!(result.simulation.summary.initial_boards, 2);
    assert_eq!(result.steps.len(), 8);
}

#[test]
fn respects_explicit_dependencies() {
    let (model, plan, deps, opts) = fixture("normal");
    let result = schedule_production(&model, &plan, &deps, opts, 30_000).unwrap();
    let a = result.ordered_source_indices.iter().position(|&i| i == 3).unwrap();
    let b = result.ordered_source_indices.iter().position(|&i| i == 1).unwrap();
    assert!(a < b);
}

#[test]
fn rejects_four_alternating_guillotine_stages() {
    let (model, plan, deps, opts) = fixture("stages");
    let err = schedule_production(&model, &plan, &deps, opts, 30_000).unwrap_err();
    assert!(matches!(err, SchedulerError::NoFeasibleOrder { .. }));
}

#[test]
fn rejects_cyclic_dependencies() {
    let (model, plan, deps, opts) = fixture("cycle");
    let err = schedule_production(&model, &plan, &deps, opts, 30_000).unwrap_err();
    assert!(matches!(err, SchedulerError::NoFeasibleOrder { .. }));
}

#[test]
fn requires_full_terminal_state_when_configured() {
    let (model, plan, deps, opts) = fixture("terminal");
    let result = schedule_production(&model, &plan, &deps, opts, 30_000).unwrap();
    assert_eq!(result.simulation.summary.live_workpieces, 0);
    assert_eq!(result.simulation.summary.scrap_workpieces, 2);
    assert_eq!(result.cut_passes, 1);
}

#[test]
fn rejects_invalid_dependency_indices_and_duplicates() {
    let (model, plan, _, opts) = fixture("normal");
    assert!(matches!(schedule_production(&model, &plan,
        &[ScheduleDependency { before: 100, after: 1 }], opts, 30_000),
        Err(SchedulerError::InvalidInput { .. })));
    assert!(matches!(schedule_production(&model, &plan,
        &[ScheduleDependency { before: 1, after: 1 }], opts, 30_000),
        Err(SchedulerError::InvalidInput { .. })));
    let same = ScheduleDependency { before: 1, after: 2 };
    assert!(matches!(schedule_production(&model, &plan, &[same.clone(),same], opts, 30_000),
        Err(SchedulerError::InvalidInput { .. })));
}

#[test]
fn rejects_search_settings_out_of_bounds() {
    let (model, plan, deps, mut opts) = fixture("normal");
    opts.max_search_nodes = 0;
    assert!(matches!(schedule_production(&model, &plan, &deps, opts, 30_000),
        Err(SchedulerError::InvalidInput { .. })));
    assert!(matches!(schedule_production(&model, &plan, &deps, ScheduleOptions::default(), 1),
        Err(SchedulerError::InvalidInput { .. })));
}

#[test]
fn deterministic_order_for_same_input() {
    let (model, plan, deps, opts) = fixture("normal");
    let a = schedule_production(&model, &plan, &deps, opts, 30_000).unwrap();
    let b = schedule_production(&model, &plan, &deps, opts, 30_000).unwrap();
    assert_eq!(a.ordered_source_indices, b.ordered_source_indices);
    assert_eq!(a.estimated_operation_cost_minor, b.estimated_operation_cost_minor);
}

#[test]
fn rejects_cut_that_cannot_be_replayed() {
    let (model, mut plan, deps, opts) = fixture("terminal");
    plan.operations.push(ProductionOperation::CutWorkpiece {
        workpiece_id: "never_existing".into(), direction: optimizer_core::model::CutDirection::Vertical,
        at: 200, first_id: "one".into(), second_id: "two".into(),
    });
    assert!(matches!(schedule_production(&model, &plan, &deps, opts, 30_000),
        Err(SchedulerError::NoFeasibleOrder { .. })));
}

#[test]
fn preserves_all_original_operations_once() {
    let (model, plan, deps, opts) = fixture("normal");
    let result = schedule_production(&model, &plan, &deps, opts, 30_000).unwrap();
    let mut sorted = result.ordered_source_indices;
    sorted.sort_unstable();
    assert_eq!(sorted, (0..plan.operations.len()).collect::<Vec<_>>());
}

#[test]
fn estimates_cost_using_custom_costs() {
    let (model, plan, deps, mut opts) = fixture("normal");
    opts.saw_pass_cost_minor = 1000;
    opts.stack_cost_minor = 500;
    opts.unstack_cost_minor = 300;
    opts.rotate_cost_minor = 10;
    opts.move_cost_minor = 20;
    let result = schedule_production(&model, &plan, &deps, opts, 30_000).unwrap();
    assert_eq!(result.estimated_operation_cost_minor, "2830");
}
