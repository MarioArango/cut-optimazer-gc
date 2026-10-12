use std::path::PathBuf;
use std::process::Command;

#[test]
fn cli_generates_three_stage_vhv_tree() {
    let fixture=PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/generate-three-stage-vhv.json");
    let result=Command::new(env!("CARGO_BIN_EXE_optimizer-cli"))
        .args(["run", "--input"]).arg(fixture).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let body:serde_json::Value=serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(body["output"]["type"], "three_stage_generation");
    assert_eq!(body["output"]["result"]["complete_order"], true);
    assert_eq!(body["output"]["result"]["board"]["evaluated"]["summary"]["stages_used"], 3);
}
