use std::process::Command;

#[test]
fn cli_plans_identical_complete_boards() {
    let binary = env!("CARGO_BIN_EXE_optimizer-cli");
    let input = format!("{}/../../examples/plan-full-board-stacks.json", env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(binary).args(["run", "--input", &input]).output().expect("CLI executes");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["output"]["type"], "full_board_stack_planning");
    assert_eq!(response["output"]["result"]["boards_in_stacks"], 3);
    assert_eq!(response["output"]["result"]["physical_execution_validated"], false);
}

#[test]
fn cli_supports_combined_geometry_and_stack_plan() {
    let binary = env!("CARGO_BIN_EXE_optimizer-cli");
    let input = format!("{}/../../examples/generate-stacked-multi-board.json", env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(binary).args(["run", "--input", &input]).output().expect("CLI executes");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["output"]["type"], "stacked_multi_board_generation");
    assert_eq!(response["output"]["result"]["allocation"]["complete_order"], true);
}
