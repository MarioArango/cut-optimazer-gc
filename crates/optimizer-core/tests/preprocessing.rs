//! Stage 3: structural input + early individual-piece fit, not a cutting plan.
use optimizer_core::{parse_request, preprocess_model, run_json, run_request, EngineError, JobOperation, JobOutput};
use optimizer_core::model::{Axis, CuttingJobModel, TrimMargins};
use optimizer_core::preprocess::{Eligibility, IneligibilityCode, PreprocessError, MAX_EXPANDED_PIECES};
use serde_json::{json, Value};

const DEMO: &str = include_str!("../../../examples/preprocess-model.json");

fn base() -> CuttingJobModel {
    let req = parse_request(DEMO).unwrap();
    let JobOperation::PreprocessModel { model } = req.operation else { panic!("incorrect operation") };
    model
}

#[test]
fn standard_melamina_preparation_has_expected_fit_choices() {
    let prepared = preprocess_model(&base()).unwrap();
    let summary = &prepared.summary;
    assert_eq!(summary.material_groups, 1);
    assert_eq!(summary.part_types, 2);
    assert_eq!(summary.expanded_pieces, 12);
    assert_eq!(summary.eligible_instances, 12);
    assert_eq!(summary.eligible_part_types, 2);
    assert_eq!(summary.ineligible_part_types, 0);
    assert_eq!(summary.stock_orientations, 2);
    assert_eq!(summary.fit_options, 3);
    assert_eq!(summary.available_stock_units, 10);
    assert!(summary.all_parts_individually_fit);
    assert!(summary.aggregate_area_sufficient);
    assert!(!summary.geometry_validated && !summary.production_validated);
    assert!(prepared.warnings.is_empty());
    assert_eq!(prepared.parts[0].fit_options.len(), 1);
    assert_eq!(prepared.parts[1].fit_options.len(), 2);
    assert_eq!(prepared.groups[0].requested_area_sq_units, "288000000");
    assert_eq!(prepared.groups[0].available_usable_area_sq_units, "4380200000");
}

#[test]
fn piece_instance_identifiers_are_unique_and_deterministic() {
    let prepared = preprocess_model(&base()).unwrap();
    assert_eq!(prepared.instances.first().unwrap().instance_id, "p-puerta#1");
    assert_eq!(prepared.instances[3].instance_id, "p-puerta#4");
    assert_eq!(prepared.instances[4].instance_id, "p-estante#1");
    assert_eq!(prepared.instances.last().unwrap().instance_id, "p-estante#8");
    let ids: std::collections::HashSet<_> = prepared.instances.iter().map(|p| p.instance_id.as_str()).collect();
    assert_eq!(ids.len(), prepared.instances.len());
}

#[test]
fn input_model_remains_unchanged_after_preprocessing() {
    let model = base();
    let copy = model.clone();
    preprocess_model(&model).unwrap();
    assert_eq!(model, copy);
}

#[test]
fn exact_fit_without_saw_clearance_is_dimensionally_eligible() {
    // Single piece fit does not require an extra kerf at the sheet's boundary.
    // This is NOT a guarantee that the geometry can be cut by the machine.
    let mut model = base();
    model.machine.allow_board_rotation = false;
    let p = &mut model.parts[0];
    p.width = 24200;
    p.height = 18100;
    let prepared = preprocess_model(&model).unwrap();
    assert_eq!(prepared.parts[0].eligibility, Eligibility::Eligible);
    assert_eq!(prepared.parts[0].fit_options.len(), 1);
}

#[test]
fn physically_rotated_stock_maps_asymmetric_trim_and_grain() {
    let mut model = base();
    model.stocks[0].trim = TrimMargins { top: 10, right: 20, bottom: 30, left: 40 };
    let prepared = preprocess_model(&model).unwrap();
    let normal = &prepared.stocks[0].orientations[0];
    let rotated = &prepared.stocks[0].orientations[1];
    assert_eq!((normal.usable_x, normal.usable_y), (40, 10));
    assert_eq!((normal.usable_width, normal.usable_height), (24340, 18260));
    assert_eq!((rotated.width, rotated.height), (18300, 24400));
    assert_eq!((rotated.usable_x, rotated.usable_y), (30, 40));
    assert_eq!((rotated.usable_width, rotated.usable_height), (18260, 24340));
    assert_eq!(normal.grain_axis, Some(Axis::X));
    assert_eq!(rotated.grain_axis, Some(Axis::Y));
}

#[test]
fn no_board_rotation_means_one_stock_orientation() {
    let mut model = base();
    model.machine.allow_board_rotation = false;
    let prepared = preprocess_model(&model).unwrap();
    assert_eq!(prepared.summary.stock_orientations, 1);
    assert_eq!(prepared.parts[1].fit_options.len(), 1);
}

#[test]
fn grain_mismatch_is_reported_not_silently_reoriented() {
    let mut model = base();
    model.machine.allow_board_rotation = false;
    model.parts[0].grain_axis = Some(Axis::Y);
    let prepared = preprocess_model(&model).unwrap();
    let door = &prepared.parts[0];
    assert_eq!(door.eligibility, Eligibility::Ineligible);
    assert_eq!(door.issue.as_ref().unwrap().code, IneligibilityCode::GrainConflict);
    assert!(!prepared.instances[0].eligible);
    assert_eq!(prepared.instances.len(), 12);
}

#[test]
fn rotation_can_make_dimensions_fit_but_must_preserve_grain() {
    let mut model = base();
    model.machine.allow_board_rotation = false;
    model.parts[0].width = 18000;
    model.parts[0].height = 24000;
    model.parts[0].allow_rotate_90 = true;
    // Only by rotating the part does it fit usable 24200 x 18100, but its
    // grain changes to Y, conflicting with sheet grain X.
    let prepared = preprocess_model(&model).unwrap();
    assert_eq!(prepared.parts[0].issue.as_ref().unwrap().code, IneligibilityCode::GrainConflict);
    model.parts[0].grain_axis = Some(Axis::Y);
    let prepared = preprocess_model(&model).unwrap();
    assert!(prepared.parts[0].fit_options.iter().any(|f| f.part_rotated_90));
}

#[test]
fn oversize_piece_is_reported_without_stopping_other_pieces() {
    let req = parse_request(include_str!("../../../examples/preprocess-ineligible-piece.json")).unwrap();
    let JobOperation::PreprocessModel { model } = req.operation else { panic!() };
    let prepared = preprocess_model(&model).unwrap();
    assert_eq!(prepared.summary.ineligible_part_types, 1);
    assert_eq!(prepared.parts[0].issue.as_ref().unwrap().code, IneligibilityCode::DoesNotFitAnyStock);
    assert_eq!(prepared.parts[1].eligibility, Eligibility::Eligible);
    assert_eq!(prepared.summary.expanded_pieces, 12);
}

#[test]
fn below_machine_minimum_detected() {
    let mut model = base();
    model.parts[0].width = 100;
    let prepared = preprocess_model(&model).unwrap();
    assert_eq!(prepared.parts[0].issue.as_ref().unwrap().code, IneligibilityCode::BelowMachineMinimum);
}

#[test]
fn missing_stock_for_material_returns_specific_issue() {
    let mut model = base();
    let mut material = model.materials[0].clone();
    material.id = "second-material".into();
    model.materials.push(material);
    model.parts[0].material_id = "second-material".into();
    let prepared = preprocess_model(&model).unwrap();
    assert_eq!(prepared.parts[0].issue.as_ref().unwrap().code, IneligibilityCode::NoStockForMaterial);
    assert_eq!(prepared.summary.material_groups, 2);
    assert_eq!(prepared.groups[1].stock_ids.len(), 0);
}

#[test]
fn aggregate_area_shortage_is_warning_not_individual_fit_failure() {
    let req = parse_request(include_str!("../../../examples/preprocess-insufficient-stock.json")).unwrap();
    let JobOperation::PreprocessModel { model } = req.operation else { panic!() };
    let prepared = preprocess_model(&model).unwrap();
    assert!(prepared.summary.all_parts_individually_fit);
    assert!(!prepared.summary.aggregate_area_sufficient);
    assert_eq!(prepared.warnings[0].code, "insufficient_stock_area");
    assert_eq!(prepared.summary.expanded_pieces, 34);
}

#[test]
fn zero_stock_inventory_for_specific_material_cannot_pass_structure_validation() {
    let mut model = base();
    model.stocks[0].quantity = 0;
    assert!(matches!(preprocess_model(&model), Err(PreprocessError::InvalidModel(_))));
}

#[test]
fn very_large_quantity_is_rejected_before_expansion() {
    let mut model = base();
    model.parts[0].quantity = MAX_EXPANDED_PIECES as u32;
    let err = preprocess_model(&model).unwrap_err();
    assert!(matches!(err, PreprocessError::ResourceLimit { resource: "expanded_pieces", .. }));
}

#[test]
fn request_json_additive_operation_is_stable() {
    let parsed: Value = serde_json::from_str(&run_json(DEMO).unwrap()).unwrap();
    assert_eq!(parsed["output"]["type"], "model_preprocessing");
    assert_eq!(parsed["output"]["prepared"]["summary"]["expanded_pieces"], 12);
    assert_eq!(parsed["output"]["prepared"]["summary"]["fit_options"], 3);
    assert_eq!(parsed["status"], "completed");
    let response = run_request(parse_request(DEMO).unwrap()).unwrap();
    assert!(matches!(response.output, JobOutput::ModelPreprocessing { .. }));
}

#[test]
fn repeated_requests_produce_identical_serialization() {
    assert_eq!(run_json(DEMO).unwrap(), run_json(DEMO).unwrap());
}

#[test]
fn invalid_model_is_rejected_at_public_boundary() {
    let mut v: Value = serde_json::from_str(DEMO).unwrap();
    v["operation"]["model"]["parts"][0]["material_id"] = json!("missing");
    assert!(matches!(parse_request(&v.to_string()), Err(EngineError::InvalidModel(_))));
}

#[test]
fn fractional_internal_dimensions_are_not_accepted() {
    let mut v: Value = serde_json::from_str(DEMO).unwrap();
    v["operation"]["model"]["parts"][0]["width"] = json!(100.5);
    assert!(matches!(parse_request(&v.to_string()), Err(EngineError::InvalidJson(_))));
}

#[test]
fn only_different_materials_do_not_form_same_group() {
    let mut model = base();
    let mut second_material = model.materials[0].clone();
    second_material.id = "other".into();
    model.materials.push(second_material);
    let mut second_stock = model.stocks[0].clone();
    second_stock.id = "other-stock".into();
    second_stock.material_id = "other".into();
    model.stocks.push(second_stock);
    model.parts[1].material_id = "other".into();
    let prepared = preprocess_model(&model).unwrap();
    assert_eq!(prepared.groups.len(), 2);
    assert_eq!(prepared.groups[0].material_id, "mel-blanca-18");
    assert_eq!(prepared.groups[0].part_ids, vec!["p-puerta".to_owned()]);
    assert_eq!(prepared.groups[1].part_ids, vec!["p-estante".to_owned()]);
}

#[test]
fn stock_kind_and_unit_cost_are_preserved() {
    let model = base();
    let prepared = preprocess_model(&model).unwrap();
    assert_eq!(prepared.stocks[0].kind, model.stocks[0].kind);
    assert_eq!(prepared.stocks[0].unit_cost_minor, 18000);
}

#[test]
fn demo_result_matches_reference_json() {
    let actual: Value = serde_json::from_str(&run_json(DEMO).unwrap()).unwrap();
    let expected: Value = serde_json::from_str(include_str!("../../../examples/expected-preprocessing.json")).unwrap();
    assert_eq!(actual, expected);
}
