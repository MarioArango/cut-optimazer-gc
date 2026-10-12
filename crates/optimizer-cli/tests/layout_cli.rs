//! Stage 6 CLI smoke test, including valid and invalid layouts.
use serde_json::Value;
use std::process::Command;

#[test]
fn valid_layout_is_reported_successful() {
    let output=Command::new(env!("CARGO_BIN_EXE_optimizer-cli"))
        .args(["run","--input",concat!(env!("CARGO_MANIFEST_DIR"),"/../../examples/layout-valid.json")])
        .output().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    let json:Value=serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["output"]["type"],"layout_validation");
    assert_eq!(json["output"]["report"]["valid"],true);
    assert_eq!(json["output"]["report"]["summary"]["verified_placed_pieces"],2);
}
#[test]
fn geometrically_invalid_layout_returns_structured_failure_report() {
    let output=Command::new(env!("CARGO_BIN_EXE_optimizer-cli"))
        .args(["run","--input",concat!(env!("CARGO_MANIFEST_DIR"),"/../../examples/layout-invalid-piece-size.json")])
        .output().unwrap();
    assert!(output.status.success());
    let json:Value=serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["output"]["report"]["valid"],false);
    assert_eq!(json["output"]["report"]["summary"]["boards_geometry_verified"],0);
}
