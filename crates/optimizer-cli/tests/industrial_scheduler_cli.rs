use std::path::PathBuf;
use std::process::Command;

fn call(name: &str) -> std::process::Output {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name);
    Command::new(env!("CARGO_BIN_EXE_optimizer-cli"))
        .args(["run", "--input", input.to_str().unwrap()])
        .output().expect("CLI launches")
}
#[test]
fn schedules_unordered_cut_and_stack_plan() {
    let out = call("schedule-production.json");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["output"]["type"], "industrial_scheduling");
    assert_eq!(json["output"]["result"]["simulation"]["summary"]["area_conserved"], true);
    assert_eq!(json["output"]["result"]["cut_passes"], 2);
}
#[test]
fn rejects_impossible_four_stage_pattern() {
    let out = call("schedule-production-invalid-stages.json");
    assert!(!out.status.success());
    let msg = String::from_utf8_lossy(&out.stderr);
    assert!(msg.contains("no feasible") || msg.contains("stage"), "{msg}");
}
#[test]
fn schedules_terminal_production() {
    let out = call("schedule-production-terminal.json");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["output"]["result"]["simulation"]["summary"]["live_workpieces"], 0);
}
