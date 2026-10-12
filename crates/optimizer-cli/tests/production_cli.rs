use std::process::Command;
use std::path::PathBuf;
fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join("examples") }
#[test]
fn simulation_cli_json_trace() {
    let path = root().join("simulate-production-stack-cut.json");
    let output = Command::new(env!("CARGO_BIN_EXE_optimizer-cli"))
        .args(["run", "--input", path.to_str().unwrap()]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["output"]["type"],"production_simulation");
    assert_eq!(result["output"]["result"]["summary"]["area_conserved"],true);
    assert_eq!(result["output"]["result"]["summary"]["saw_passes"],2);
}
#[test]
fn malformed_cut_is_rejected_by_cli() {
    let path = root().join("simulate-production-invalid-cut.json");
    let output = Command::new(env!("CARGO_BIN_EXE_optimizer-cli"))
        .args(["run", "--input", path.to_str().unwrap()]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cut_geometry"));
}
