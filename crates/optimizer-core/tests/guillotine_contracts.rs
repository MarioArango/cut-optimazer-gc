//! Stage 5 public JSON contract and cutting-tree invariants.
use optimizer_core::{capabilities, parse_request, run_json, run_request, EngineError, JobOutput};
use optimizer_core::{EvaluatedNode, TreeError};
use serde_json::{json, Value};

const VH: &str = include_str!("../../../examples/guillotine-vh.json");
const HV: &str = include_str!("../../../examples/guillotine-hv.json");
const VHV: &str = include_str!("../../../examples/guillotine-vhv.json");
const HVH: &str = include_str!("../../../examples/guillotine-hvh.json");

fn fail_code(text: &str) -> String {
    match parse_request(text).unwrap_err() {
        EngineError::Guillotine(TreeError { code, .. }) => code,
        other => panic!("expected guillotine tree validation error; got {other:?}"),
    }
}
fn change<F: FnOnce(&mut Value)>(fixture: &str, edit: F) -> String {
    let mut input: Value = serde_json::from_str(fixture).unwrap();
    edit(&mut input);
    input.to_string()
}
#[test]
fn capabilities_include_tree_evaluation_but_no_optimizer() {
    let caps=capabilities();
    assert_eq!(caps.stage, 18);
    assert!(caps.supported_operations.contains(&"guillotine_evaluate".to_string()));
    assert!(caps.optimization_available);
}
#[test]
fn all_four_direction_sequences_evaluate() {
    for fixture in [VH,HV,VHV,HVH] {
        let result=run_request(parse_request(fixture).unwrap()).unwrap();
        let JobOutput::GuillotineEvaluation { evaluated } = result.output else { panic!("wrong output") };
        assert_eq!(evaluated.summary.cuts, if fixture==VHV {4} else if fixture==HVH {3} else {2});
        assert_eq!(evaluated.summary.stages_used, if fixture==VHV||fixture==HVH {3} else {2});
        let summary=&evaluated.summary;
        let parse_area=|s:&String|s.parse::<u128>().unwrap();
        assert_eq!(parse_area(&summary.usable_area_sq_units), parse_area(&summary.piece_area_sq_units)+parse_area(&summary.residual_area_sq_units)+parse_area(&summary.kerf_area_sq_units));
        assert!(!summary.production_validated);
        assert!(matches!(evaluated.root,EvaluatedNode::Split{stage:1,..}));
    }
}
#[test]
fn vhv_exact_coordinates_and_per_part_counts() {
    let result=run_request(parse_request(VHV).unwrap()).unwrap();
    let JobOutput::GuillotineEvaluation { evaluated:e } = result.output else { panic!("wrong output") };
    assert_eq!((e.usable_rect.x,e.usable_rect.y,e.usable_rect.width,e.usable_rect.height),(10,10,980,780));
    assert_eq!(e.summary.cuts,4);
    assert_eq!(e.summary.placed_pieces,3);
    assert_eq!(e.summary.residual_regions,2);
    assert_eq!(e.summary.usable_area_sq_units,"764400");
    assert_eq!(e.summary.piece_area_sq_units,"288000");
    assert_eq!(e.summary.residual_area_sq_units,"455900");
    assert_eq!(e.summary.kerf_area_sq_units,"20500");
    assert_eq!(e.summary.per_part_counts.get("A"),Some(&1));
    assert_eq!(e.summary.per_part_counts.get("B"),Some(&1));
    assert_eq!(e.summary.per_part_counts.get("C"),Some(&1));
    assert_eq!((e.geometric_cuts[0].parent.x,e.geometric_cuts[0].saw_path.x),(10,410));
    assert_eq!((e.geometric_cuts[3].first.width,e.geometric_cuts[3].second.width),(250,310));
    assert_eq!((e.geometric_cuts[3].stage,e.geometric_cuts[3].direction), (3,optimizer_core::model::CutDirection::Vertical));
}
#[test]
fn saw_channels_do_not_overlap_piece_or_residual() {
    let result=run_request(parse_request(VHV).unwrap()).unwrap();
    let JobOutput::GuillotineEvaluation { evaluated:e }=result.output else {panic!("missing")};
    let mut all:Vec<_>=e.pieces.iter().map(|p|p.rect).collect();
    all.extend(e.residuals.iter().map(|r|r.rect));
    all.extend(e.geometric_cuts.iter().map(|c|c.saw_path));
    for i in 0..all.len() {
        for j in i+1..all.len() {
            assert!(!all[i].intersects(&all[j]).unwrap(),"regions {i} {j} overlap");
        }
    }
}
#[test]
fn board_rotation_rotates_margins_and_dimensions() {
    let x=include_str!("../../../examples/guillotine-rotated-board.json");
    let result=run_request(parse_request(x).unwrap()).unwrap();
    let JobOutput::GuillotineEvaluation { evaluated:e }=result.output else {panic!("missing")};
    assert!(e.board_rotated_90);
    assert_eq!((e.usable_rect.width,e.usable_rect.height),(780,980));
    assert_eq!(e.summary.cuts,0);
    assert_eq!(e.summary.stages_used,0);
}
#[test]
fn wrong_initial_direction_rejected() {
    assert_eq!(fail_code(include_str!("../../../examples/guillotine-invalid-wrong-direction.json")),"invalid_stage_transition");
}
#[test]
fn saw_channel_cannot_exit_board() {
    assert_eq!(fail_code(include_str!("../../../examples/guillotine-invalid-invalid-kerf.json")),"invalid_split");
}
#[test]
fn piece_must_fill_exact_leaf_rectangle() {
    assert_eq!(fail_code(include_str!("../../../examples/guillotine-invalid-mismatched-piece.json")),"piece_dimension_mismatch");
}
#[test]
fn nonexistent_pattern_rejected() {
    assert_eq!(fail_code(include_str!("../../../examples/guillotine-invalid-unknown-pattern.json")),"unknown_pattern");
}
#[test]
fn machine_cut_length_respected() {
    assert_eq!(fail_code(include_str!("../../../examples/guillotine-invalid-long-cut.json")),"cut_too_long");
}
#[test]
fn part_rotation_permission_enforced() {
    let invalid=change(VHV,|v|v["operation"]["model"]["patterns"][0]["root"]["first"]["first"]["rotated"]=json!(true));
    assert_eq!(fail_code(&invalid),"rotation_forbidden");
}
#[test]
fn grain_consistency_enforced() {
    let invalid=change(VHV,|v| {
        v["operation"]["model"]["parts"][0]["allow_rotate_90"]=json!(true);
        v["operation"]["model"]["patterns"][0]["root"]["first"]["first"]["rotated"]=json!(true);
    });
    // The rotated 400x300 piece would be 300x400, so dimensions fail before grain.
    assert_eq!(fail_code(&invalid),"piece_dimension_mismatch");
}
#[test]
fn grain_conflict_even_with_equal_square_dimensions() {
    let invalid=change(VHV,|v| {
        v["operation"]["model"]["parts"][0]["width"]=json!(400);
        v["operation"]["model"]["parts"][0]["height"]=json!(400);
        v["operation"]["model"]["parts"][0]["allow_rotate_90"]=json!(true);
        v["operation"]["model"]["patterns"][0]["root"]["first"]["first"]["rotated"]=json!(true);
        // Use different tree with matching 400x400 leaf (and residual elsewhere).
        v["operation"]["model"]["patterns"][0]["root"]["first"]["at"]=json!(400);
    });
    assert_eq!(fail_code(&invalid),"grain_conflict");
}
#[test]
fn board_rotation_requires_machine_permission() {
    let invalid=change(include_str!("../../../examples/guillotine-rotated-board.json"),|v|
        v["operation"]["model"]["machine"]["allow_board_rotation"]=json!(false));
    assert_eq!(fail_code(&invalid),"board_rotation_forbidden");
}
#[test]
fn invalid_fourth_stage_is_rejected() {
    let invalid=change(VHV,|v| {
        // On V-H-V right/top/right leaf replace the piece with an H split.
        // Stage check executes before any piece-size matching.
        v["operation"]["model"]["patterns"][0]["root"]["second"]["first"]["second"] =json!({
            "kind":"split","node_id":"n-fourth","direction":"horizontal","at":100,
            "first":{"kind":"residual","node_id":"r-new-1","reusable":false},
            "second":{"kind":"residual","node_id":"r-new-2","reusable":false}
        });
    });
    assert_eq!(fail_code(&invalid),"invalid_stage_transition");
}
#[test]
fn dimensions_below_machine_minimum_rejected() {
    let invalid=change(VHV,|v|v["operation"]["model"]["machine"]["min_piece_width"]=json!(405));
    assert_eq!(fail_code(&invalid),"piece_below_machine_minimum");
}
#[test]
fn duplicate_piece_use_beyond_quantity_rejected() {
    let invalid=change(VHV,|v| {
        let op=&mut v["operation"];
        op["model"]["parts"][0]["width"]=json!(485);
        op["model"]["parts"][0]["height"]=json!(780);
        op["model"]["parts"][0]["quantity"]=json!(1);
        op["model"]["patterns"][0]["root"]=json!({
            "kind":"split","node_id":"root-two","direction":"vertical","at":485,
            "first":{"kind":"piece","node_id":"pa","part_id":"A","rotated":false},
            "second":{"kind":"piece","node_id":"pb","part_id":"A","rotated":false}
        });
    });
    assert_eq!(fail_code(&invalid),"quantity_exceeded");
}
#[test]
fn modified_guillotine_evaluation_survives_json_roundtrip() {
    let request=parse_request(VHV).unwrap();
    assert_eq!(request,parse_request(&serde_json::to_string(&request).unwrap()).unwrap());
    let json:Value=serde_json::from_str(&run_json(VHV).unwrap()).unwrap();
    assert_eq!(json["output"]["type"],"guillotine_evaluation");
    assert_eq!(json["output"]["evaluated"]["summary"]["production_validated"],false);
}
