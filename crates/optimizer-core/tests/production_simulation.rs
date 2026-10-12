use optimizer_core::{
    simulate_production, InitialBoard, PhysicalState, ProductionOperation,
    ProductionPlan, SimulationError,
};
use optimizer_core::model::{CutDirection, CuttingJobModel};
use serde_json::json;

fn model() -> CuttingJobModel {
    let request: serde_json::Value = serde_json::from_str(include_str!("../../../examples/plan-full-board-stacks.json")).unwrap();
    serde_json::from_value(request["operation"]["model"].clone()).unwrap()
}
fn board(id: &str, index: u32) -> InitialBoard {
    InitialBoard { board_id: id.into(), stock_id: "board1000".into(), stock_unit_index: index, rotated_90: false }
}
fn run(ops: Vec<ProductionOperation>) -> Result<optimizer_core::ProductionSimulation, SimulationError> {
    simulate_production(&model(), &ProductionPlan {
        initial_boards: vec![board("b0", 0), board("b1", 1)], operations: ops,
    })
}
fn stack() -> ProductionOperation {
    ProductionOperation::Stack { stack_id: "whole".into(), member_ids: vec!["b0".into(),"b1".into()] }
}
fn cut_stack() -> ProductionOperation {
    ProductionOperation::CutStack {
        stack_id: "whole".into(), direction: CutDirection::Vertical, at: 400,
        first_stack_id: "left".into(), second_stack_id: "right".into(),
        first_member_ids: vec!["b0_left".into(),"b1_left".into()],
        second_member_ids: vec!["b0_right".into(),"b1_right".into()],
    }
}
fn assert_code(err: SimulationError, code: &str) {
    match err { SimulationError::InvalidPlan { code: actual, .. } => assert_eq!(actual,code), x => panic!("unexpected error: {x:?}") }
}

#[test]
fn package_cut_creates_two_packages_preserves_every_layer_and_kerf() {
    let result = run(vec![stack(),cut_stack()]).unwrap();
    assert_eq!(result.summary.saw_passes, 1);
    assert_eq!(result.summary.active_stacks, 2);
    assert_eq!(result.summary.live_workpieces, 4);
    assert_eq!(result.summary.initial_area_sq_units,"1600000");
    assert_eq!(result.summary.kerf_area_sq_units,"16000");
    assert!(result.summary.area_conserved);
    let l = result.workpieces.iter().find(|p| p.id=="b0_left").unwrap();
    let r = result.workpieces.iter().find(|p| p.id=="b1_right").unwrap();
    assert_eq!((l.width,l.height),(400,800));
    assert_eq!((r.width,r.height),(590,800));
    assert_eq!(l.origin_board_id,"b0");
    assert_eq!(r.origin_board_id,"b1");
    assert_eq!(l.parent_id.as_deref(),Some("b0"));
    assert_eq!(r.stack_id.as_deref(),Some("right"));
    assert_eq!(result.events[1].kerf_area_sq_units,"16000");
}

#[test]
fn unstack_cut_finish_scrap_trace_and_area() {
    let result = run(vec![
        stack(), cut_stack(),
        ProductionOperation::Unstack { stack_id: "left".into() },
        ProductionOperation::CutWorkpiece { workpiece_id:"b0_left".into(), direction:CutDirection::Horizontal,
            at:300,first_id:"p0".into(),second_id:"offcut0".into() },
        ProductionOperation::Finish { workpiece_id:"p0".into(),part_id:"A".into(),rotated:false },
        ProductionOperation::Scrap { workpiece_id:"offcut0".into(),reusable:true },
        ProductionOperation::MoveStack {stack_id:"right".into(), station:"outfeed".into()},
        ProductionOperation::RotateStack {stack_id:"right".into()},
    ]).unwrap();
    assert!(result.summary.area_conserved);
    assert_eq!(result.summary.saw_passes,2);
    assert_eq!(result.summary.kerf_area_sq_units,"20000");
    assert_eq!(result.summary.finished_per_part["A"],1);
    assert_eq!(result.summary.scrap_workpieces,1);
    assert_eq!(result.summary.active_stacks,1);
    let rotated = result.workpieces.iter().find(|p|p.id=="b1_right").unwrap();
    assert_eq!((rotated.width,rotated.height),(800,590));
    assert_eq!(rotated.station,"outfeed");
    assert_eq!(result.workpieces.iter().find(|p|p.id=="p0").unwrap().state,PhysicalState::Finished);
}

#[test]
fn duplicate_stock_unit_rejected() {
    let err = simulate_production(&model(), &ProductionPlan {
        initial_boards:vec![board("a",0),board("b",0)], operations:vec![],
    }).unwrap_err();
    assert_code(err,"duplicate_stock_unit");
}
#[test]
fn impossible_cut_rejected() {
    let err = run(vec![ProductionOperation::CutWorkpiece {workpiece_id:"b0".into(),
        direction:CutDirection::Vertical,at:996,first_id:"a".into(),second_id:"c".into()}]).unwrap_err();
    assert_code(err,"cut_geometry");
}
#[test]
fn double_use_of_workpiece_after_cut_rejected() {
    let err = run(vec![stack(),cut_stack(),ProductionOperation::RotateWorkpiece{workpiece_id:"b0".into()}]).unwrap_err();
    assert_code(err,"unavailable_workpiece");
}
#[test]
fn cannot_cut_member_before_unstack() {
    let err = run(vec![stack(),ProductionOperation::CutWorkpiece{workpiece_id:"b0".into(),
        direction:CutDirection::Vertical,at:400,first_id:"l".into(),second_id:"r".into()}]).unwrap_err();
    assert_code(err,"stacked_workpiece");
}
#[test]
fn package_height_rejected() {
    let mut m = model(); m.machine.max_stack_height = 180;
    let err = simulate_production(&m, &ProductionPlan {initial_boards:vec![board("b0",0),board("b1",1)],
        operations:vec![stack()] }).unwrap_err();
    assert_code(err,"stack_height");
}
#[test]
fn different_dimensions_cannot_stack() {
    let err = run(vec![ProductionOperation::CutWorkpiece {workpiece_id:"b0".into(),
        direction:CutDirection::Vertical,at:400,first_id:"a".into(),second_id:"r".into()},
        ProductionOperation::Stack{stack_id:"bad".into(),member_ids:vec!["a".into(),"b1".into()]}]).unwrap_err();
    assert_code(err,"incompatible_stack");
}
#[test]
fn overproduction_rejected() {
    let mut m = model(); m.parts[0].quantity = 0;
    // model validation rejects zero before production; no silent false positives.
    let err = simulate_production(&m, &ProductionPlan {initial_boards:vec![board("b0",0)],operations:vec![]}).unwrap_err();
    assert!(matches!(err, SimulationError::InvalidModel(_)));
}
#[test]
fn malformed_cut_stack_output_ids_rejected() {
    let mut op = cut_stack();
    if let ProductionOperation::CutStack { second_member_ids, .. } = &mut op {
        second_member_ids[0] = "b0_left".into();
    }
    let err = run(vec![stack(),op]).unwrap_err();
    assert_code(err,"duplicate_or_invalid_id");
}
#[test]
fn cannot_reuse_old_stack_id_after_unstack() {
    let err = run(vec![stack(), ProductionOperation::Unstack{stack_id:"whole".into()}, stack()]).unwrap_err();
    assert_code(err,"duplicate_or_invalid_stack_id");
}
#[test]
fn model_does_not_silently_trim_original_stock() {
    let result = run(vec![]).unwrap();
    assert_eq!(result.summary.initial_area_sq_units,"1600000");
}
#[test]
fn fully_deterministic_json() {
    let ops = vec![stack(),cut_stack()];
    let a = serde_json::to_string(&run(ops.clone()).unwrap()).unwrap();
    let b = serde_json::to_string(&run(ops).unwrap()).unwrap();
    assert_eq!(a,b);
}
#[test]
fn can_deserialize_serialized_operation_tag() {
    let op: ProductionOperation = serde_json::from_value(json!({"type":"unstack","stack_id":"A"})).unwrap();
    assert_eq!(op,ProductionOperation::Unstack {stack_id:"A".into()});
}
#[test]
fn move_workpiece_rejects_invalid_station() {
    let err = run(vec![ProductionOperation::MoveWorkpiece{workpiece_id:"b0".into(), station:"not valid".into()}]).unwrap_err();
    assert_code(err,"invalid_station");
}
#[test]
fn stack_cut_results_reject_second_cut_on_consumed_parent() {
    let err = run(vec![stack(),cut_stack(),cut_stack()]).unwrap_err();
    assert_code(err,"unknown_stack");
}
