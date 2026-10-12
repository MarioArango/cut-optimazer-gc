//! Rust-only checks for checked-in Postman requests and project JSON fixtures.
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_json_fixture_and_api_document_is_parseable() {
    for folder in ["examples", "postman", "docs"] {
        let location = project_root().join(folder);
        let read = fs::read_dir(&location).expect("fixture directory must exist");
        for entry in read {
            let path = entry.expect("directory entry").path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let bytes = fs::read(&path).expect("read JSON fixture");
            serde_json::from_slice::<Value>(&bytes)
                .unwrap_or_else(|err| panic!("invalid JSON fixture {}: {err}", path.display()));
        }
    }
}

#[test]
fn every_postman_job_uses_the_native_rust_json_contract() {
    let path = project_root().join("postman/Industrial-Cutting-Optimizer.postman_collection.json");
    let doc: Value = serde_json::from_slice(&fs::read(path).expect("Postman collection")).unwrap();
    let items = doc["item"].as_array().expect("Postman items");
    assert!(items.len() >= 20);
    for item in items {
        if item["request"]["method"] != "POST" {
            continue;
        }
        let raw = item["request"]["body"]["raw"].as_str().expect("Postman JSON request");
        let request: optimizer_core::JobRequest = serde_json::from_str(raw)
            .unwrap_or_else(|err| panic!("{}: {err}", item["name"]));
        assert_eq!(request.schema_version, optimizer_core::SCHEMA_VERSION);
        assert!(!request.job_id.is_empty());
    }
}

#[test]
fn documented_openapi_matches_routes() {
    let path = project_root().join("docs/openapi-stage17.json");
    let doc: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(doc["openapi"], "3.0.3");
    assert!(doc["paths"]["/api/v1/jobs"]["post"].is_object());
    assert!(doc["paths"]["/api/v1/jobs/validate"]["post"].is_object());
    assert!(doc["paths"]["/healthz"]["get"].is_object());
}
