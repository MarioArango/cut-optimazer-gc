use std::path::PathBuf;
use std::process::Command;

fn run(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name);
    let output = Command::new(env!("CARGO_BIN_EXE_optimizer-cli"))
        .args(["run", "--input", path.to_str().unwrap()])
        .output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn cli_groups_three_strips_and_preserves_area() {
    let value = run("plan-strip-stacks.json");
    assert_eq!(value["output"]["type"], "strip_stack_planning");
    assert_eq!(value["output"]["result"]["groups"].as_array().unwrap().len(), 2);
    assert_eq!(value["output"]["result"]["projected_simulation"]["summary"]["area_conserved"], true);
}

#[test]
fn cli_keeps_existing_strip_package() {
    let value = run("plan-strip-stacks-existing.json");
    assert_eq!(value["output"]["result"]["groups"][0]["origin"], "already_stacked");
    assert_eq!(value["output"]["result"]["operations_added"].as_array().unwrap().len(), 1);
}

#[test]
fn cli_disabled_returns_no_group_and_no_new_operations() {
    let value = run("plan-strip-stacks-disabled.json");
    assert_eq!(value["output"]["result"]["groups"].as_array().unwrap().len(), 0);
    assert_eq!(value["output"]["result"]["operations_added"].as_array().unwrap().len(), 0);
}
