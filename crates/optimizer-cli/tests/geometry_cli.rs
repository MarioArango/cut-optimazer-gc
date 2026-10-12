use std::path::PathBuf;
use std::process::Command;
fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name)
}
fn cli() -> Command { Command::new(env!("CARGO_BIN_EXE_optimizer-cli")) }
#[test]
fn cli_vertical_split_has_no_overlap() {
    let output = cli().args(["run","--input"]).arg(example("geometry-vertical-split.json")).output().unwrap();
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let s=&parsed["output"]["split"];
    assert_eq!(s["first"]["x"],100);
    assert_eq!(s["saw_path"]["x"],500);
    assert_eq!(s["second"]["x"],532);
}
#[test]
fn cli_trimming_works() {
    let output = cli().args(["run","--input"]).arg(example("geometry-trim.json")).output().unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["output"]["usable"]["width"],960);
}
#[test]
fn cli_invalid_cut_exit_three() {
    let output = cli().args(["run","--input"]).arg(example("geometry-invalid-cut.json")).output().unwrap();
    assert_eq!(output.status.code(),Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid geometry"));
}
#[test]
fn cli_validate_rejects_invalid_cut() {
    let output = cli().args(["validate","--input"]).arg(example("geometry-invalid-cut.json")).output().unwrap();
    assert_eq!(output.status.code(),Some(3));
}
