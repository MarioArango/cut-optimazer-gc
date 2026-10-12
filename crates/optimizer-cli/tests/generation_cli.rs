//! Executable smoke test for Stage 7 CLI generation.
use std::process::Command;
use serde_json::Value;

#[test]
fn cli_generates_one_board_with_evaluated_tree() {
    let root = env!("CARGO_MANIFEST_DIR");
    let example = format!("{root}/../../examples/generate-two-stage.json");
    let output = Command::new(env!("CARGO_BIN_EXE_optimizer-cli"))
        .args(["run", "--input", &example]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["output"]["type"], "two_stage_generation");
    assert_eq!(result["output"]["result"]["found"], true);
    assert_eq!(result["output"]["result"]["remaining_part_counts"]["A"], 0);
    assert!(result["output"]["result"]["board"]["evaluated"]["pieces"].as_array().unwrap().len() >= 2);
}
