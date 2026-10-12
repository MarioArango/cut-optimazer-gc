use std::path::PathBuf;
use std::process::Command;

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name)
}
fn cli() -> Command { Command::new(env!("CARGO_BIN_EXE_optimizer-cli")) }

#[test]
fn preprocess_cli_runs_and_returns_expanded_pieces() {
    let output = cli().args(["run", "--input"]).arg(example("preprocess-model.json")).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["output"]["type"], "model_preprocessing");
    assert_eq!(value["output"]["prepared"]["instances"].as_array().unwrap().len(), 12);
}

#[test]
fn preprocess_issues_are_returned_as_data_not_process_failure() {
    let output = cli().args(["run", "--input"]).arg(example("preprocess-ineligible-piece.json")).output().unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["output"]["prepared"]["summary"]["ineligible_part_types"], 1);
}

#[test]
fn preprocessing_capacity_error_has_validation_exit_code() {
    let source = std::fs::read_to_string(example("preprocess-model.json")).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&source).unwrap();
    json["operation"]["model"]["parts"][0]["quantity"] = serde_json::json!(100_001);
    let mut command = cli();
    let mut child = command.args(["run", "--input", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn().unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(json.to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("expanded_pieces"));
}
