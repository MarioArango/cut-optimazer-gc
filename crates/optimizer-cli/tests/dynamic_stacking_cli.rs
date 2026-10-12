use std::path::PathBuf;
use std::process::Command;

fn run(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name);
    let out = Command::new(env!("CARGO_BIN_EXE_optimizer-cli"))
        .args(["run", "--input", path.to_str().unwrap()])
        .output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn dynamic_cli_shares_multiple_cuts() {
    let value = run("plan-dynamic-stacks.json");
    assert_eq!(value["output"]["type"], "dynamic_stack_planning");
    assert_eq!(value["output"]["result"]["saw_passes_avoided"], 4);
    assert_eq!(value["output"]["result"]["projected_simulation"]["summary"]["area_conserved"], true);
}

#[test]
fn divergent_cli_includes_unstack() {
    let value = run("plan-dynamic-divergent.json");
    let ops = value["output"]["result"]["operations_added"].as_array().unwrap();
    assert!(ops.iter().any(|o| o["type"] == "unstack"));
}

#[test]
fn dynamic_cli_can_regroup_across_origins() {
    let value = run("plan-dynamic-repackage.json");
    let decisions = value["output"]["result"]["decisions"].as_array().unwrap();
    assert!(decisions.iter().any(|d| d["kind"] == "unstack_for_regroup"));
}
