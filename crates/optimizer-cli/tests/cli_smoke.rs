use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::io::Write;

fn demo_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/system-check.json")
}

fn minimal_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/system-check-minimal.json")
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_optimizer-cli"))
}

#[test]
fn validate_example_succeeds() {
    let output = cli()
        .args(["validate", "--input"])
        .arg(demo_path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", String::from_utf8_lossy(&output.stderr));
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["valid"], serde_json::Value::Bool(true));
    assert_eq!(body["job_id"], "demo-stack-001");
}

#[test]
fn run_example_succeeds() {
    let output = cli()
        .args(["run", "--input"])
        .arg(demo_path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", String::from_utf8_lossy(&output.stderr));
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["status"], "completed");
    assert_eq!(body["output"]["optimization_available"], serde_json::Value::Bool(true));
}

#[test]
fn can_read_from_stdin() {
    let mut child = cli()
        .args(["run", "--input", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let contents = std::fs::read_to_string(minimal_path()).unwrap();
    child.stdin.take().unwrap().write_all(contents.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["seed"], 42);
}

#[test]
fn malformed_input_returns_code_3() {
    let output = cli()
        .args(["run", "--input"])
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/invalid-schema.json"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn no_input_returns_code_2() {
    let output = cli().args(["run"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn capabilities_are_machine_readable() {
    let output = cli().arg("capabilities").output().unwrap();
    assert!(output.status.success());
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["stage"], 18);
}

#[test]
fn validate_model_example_runs() {
    let output = cli()
        .args(["run", "--input"])
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/validate-model.json"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["output"]["type"], "model_validation");
    assert_eq!(body["output"]["summary"]["requested_pieces"], 12);
    assert_eq!(body["output"]["summary"]["stacks"], 1);
    assert_eq!(body["output"]["summary"]["geometry_validated"], false);
}

#[test]
fn cli_rejects_invalid_model() {
    let output = cli()
        .args(["validate", "--input"])
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/invalid-model.json"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid cutting model"));
}
