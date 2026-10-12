use std::path::PathBuf;
use std::process::Command;
fn fixture(name:&str)->PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name)
}
fn cli()->Command{Command::new(env!("CARGO_BIN_EXE_optimizer-cli"))}
#[test]
fn vhv_cli_outputs_actual_tree() {
    let output=cli().args(["run","--input"]).arg(fixture("guillotine-vhv.json")).output().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    let json:serde_json::Value=serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["output"]["evaluated"]["summary"]["stages_used"],3);
    assert_eq!(json["output"]["evaluated"]["summary"]["kerf_area_sq_units"],"20500");
}
#[test]
fn invalid_tree_cli_exit_three() {
    let output=cli().args(["run","--input"]).arg(fixture("guillotine-invalid-wrong-direction.json")).output().unwrap();
    assert_eq!(output.status.code(),Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid_stage_transition"));
}
#[test]
fn validate_detects_geometric_invalidity() {
    let output=cli().args(["validate","--input"]).arg(fixture("guillotine-invalid-mismatched-piece.json")).output().unwrap();
    assert_eq!(output.status.code(),Some(3));
}
