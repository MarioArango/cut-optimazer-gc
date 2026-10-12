//! Public CLI smoke test for Stage 15.
use std::process::Command;

#[test]
fn optimized_job_is_emitted_as_json() {
    let binary = env!("CARGO_BIN_EXE_optimizer-cli");
    let input = format!("{}/../../examples/optimize-global.json", env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(binary).args(["run", "--input", &input]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["output"]["type"], "global_optimization");
    assert_eq!(body["output"]["result"]["best"]["complete_order"], true);
    assert_eq!(body["engine_version"], "0.18.0");
}
