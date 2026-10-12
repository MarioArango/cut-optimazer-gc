use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use optimizer_api::{router, ApiConfig};
use serde_json::{json, Value};
use tower::ServiceExt;

fn post(payload: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/jobs")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_owned()))
        .expect("valid request")
}

#[tokio::test]
async fn real_postman_fixture_is_accepted_as_json_contract() {
    let body = include_str!("../../../examples/postman-industrial-real.json");
    let mut request: Value = serde_json::from_str(body).unwrap();
    // Contract validation is deliberately separate from costly optimization.
    let response = router(ApiConfig::default()).oneshot(
        Request::builder().method("POST")
            .uri("/api/v1/jobs/validate")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&request).unwrap())).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["valid"], true);
    request["operation"]["max_boards"] = json!(0);
    let response = router(ApiConfig::default()).oneshot(
        Request::builder().method("POST")
            .uri("/api/v1/jobs/validate")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(request.to_string())).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn previously_supported_multi_board_fixture_still_runs() {
    let body = include_str!("../../../examples/generate-multi-board.json");
    let response = router(ApiConfig::default()).oneshot(post(body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["output"]["type"], "multi_board_generation");
    assert_eq!(payload["job_id"], "multi-board-example-001");
}

#[tokio::test]
async fn health_and_capabilities_do_not_claim_finished_order() {
    let response = router(ApiConfig::default()).oneshot(
        Request::builder().uri("/api/v1/capabilities")
            .body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["stage"], 18);
    assert_eq!(payload["engine_version"], "0.18.0");
}
