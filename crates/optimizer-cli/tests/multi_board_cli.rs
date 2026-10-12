//! End-to-end command-line invocation for Stage 9.
use std::process::Command;

#[test]
fn json_example_generates_multiple_boards() {
    let bin = env!("CARGO_BIN_EXE_optimizer-cli");
    let example = format!("{}/../../examples/generate-multi-board.json", env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(bin).args(["run", "--input", &example]).output().expect("run cli");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["output"]["type"], "multi_board_generation");
    assert_eq!(result["output"]["result"]["complete_order"], true);
    assert_eq!(result["output"]["result"]["boards"].as_array().unwrap().len(), 3);
}
