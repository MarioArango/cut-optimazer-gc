use std::process::Command;

#[test]
fn economic_job_emits_auditable_json() {
    let binary = env!("CARGO_BIN_EXE_optimizer-cli");
    let input = format!("{}/../../examples/evaluate-economics.json", env!("CARGO_MANIFEST_DIR"));
    let result = Command::new(binary).args(["run", "--input", &input]).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let data: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(data["engine_version"], "0.18.0");
    assert_eq!(data["output"]["type"], "economic_evaluation");
    assert_eq!(data["output"]["result"]["layout_geometry_validated"], true);
    assert_eq!(data["output"]["result"]["order_complete"], true);
    assert_eq!(data["output"]["result"]["separate_boards"]["material_cost_minor"], "10000");
}

#[test]
fn invalid_salvage_settings_fail_cli() {
    let binary = env!("CARGO_BIN_EXE_optimizer-cli");
    let input = format!("{}/../../examples/evaluate-economics-invalid.json", env!("CARGO_MANIFEST_DIR"));
    let result = Command::new(binary).args(["run", "--input", &input]).output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("recovery"));
}

#[test]
fn economic_comparison_selects_cheaper_layout() {
    let binary = env!("CARGO_BIN_EXE_optimizer-cli");
    let input = format!("{}/../../examples/compare-economic-layouts.json", env!("CARGO_MANIFEST_DIR"));
    let result = Command::new(binary).args(["run", "--input", &input]).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let data: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(data["output"]["type"], "economic_comparison");
    assert_eq!(data["output"]["result"]["selected_layout_index"], 1);
}
