//! Standalone JSON REST facade for the industrial optimizer.
//!
//! Runs entirely in Rust. The existing `optimizer-core` has no HTTP dependency.
//! No ERP, database, JavaScript runtime, or Node.js is required.
//!
//! Threading model: Tokio accepts connections; blocking optimization jobs run
//! on the blocking pool behind a fixed semaphore. Capacity is returned with
//! HTTP 503 rather than queuing an unbounded number of expensive searches.

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{rejection::BytesRejection, DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use optimizer_core::{capabilities, run_request, validate_request, JobOperation, JobOutput, JobRequest};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const DEFAULT_BIND: &str = "127.0.0.1:8080";
const DEFAULT_BODY_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_PARALLEL_JOBS: usize = 2;
const DEFAULT_JOB_RUNTIME_MS: u64 = 120_000;

/// Settings for the HTTP process. Defaults intentionally expose the API only
/// on loopback: binding to a public interface requires OPTIMIZER_API_KEY.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub bind: SocketAddr,
    pub api_key: Option<String>,
    pub max_body_bytes: usize,
    pub max_parallel_jobs: usize,
    pub max_job_runtime_ms: u64,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND.parse().expect("constant socket address"),
            api_key: None,
            max_body_bytes: DEFAULT_BODY_BYTES,
            max_parallel_jobs: DEFAULT_PARALLEL_JOBS,
            max_job_runtime_ms: DEFAULT_JOB_RUNTIME_MS,
        }
    }
}

impl ApiConfig {
    pub fn from_env() -> Result<Self, String> {
        let mut result = Self::default();
        if let Ok(value) = env::var("OPTIMIZER_BIND") {
            result.bind = value.parse().map_err(|_| "OPTIMIZER_BIND must be an IP:PORT".to_owned())?;
        }
        if let Ok(value) = env::var("OPTIMIZER_API_KEY") {
            result.api_key = Some(value);
        }
        if let Ok(value) = env::var("OPTIMIZER_MAX_BODY_BYTES") {
            result.max_body_bytes = value.parse().map_err(|_| "OPTIMIZER_MAX_BODY_BYTES must be an integer".to_owned())?;
        }
        if let Ok(value) = env::var("OPTIMIZER_MAX_PARALLEL_JOBS") {
            result.max_parallel_jobs = value.parse().map_err(|_| "OPTIMIZER_MAX_PARALLEL_JOBS must be an integer".to_owned())?;
        }
        if let Ok(value) = env::var("OPTIMIZER_MAX_JOB_RUNTIME_MS") {
            result.max_job_runtime_ms = value.parse().map_err(|_| "OPTIMIZER_MAX_JOB_RUNTIME_MS must be an integer".to_owned())?;
        }
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(1024..=64 * 1024 * 1024).contains(&self.max_body_bytes) {
            return Err("OPTIMIZER_MAX_BODY_BYTES must be in 1024..67108864".into());
        }
        if !(1..=64).contains(&self.max_parallel_jobs) {
            return Err("OPTIMIZER_MAX_PARALLEL_JOBS must be in 1..64".into());
        }
        if !(100..=3_600_000).contains(&self.max_job_runtime_ms) {
            return Err("OPTIMIZER_MAX_JOB_RUNTIME_MS must be in 100..3600000".into());
        }
        if self.api_key.as_ref().is_some_and(|key| key.trim().len() < 16) {
            return Err("OPTIMIZER_API_KEY must contain at least 16 non-whitespace characters".into());
        }
        if !self.bind.ip().is_loopback() && self.api_key.is_none() {
            return Err("binding outside localhost requires OPTIMIZER_API_KEY".into());
        }
        Ok(())
    }
}

struct ApiState {
    token: Option<String>,
    running: Arc<Semaphore>,
    max_job_runtime_ms: u64,
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: String,
}

struct ApiFailure {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiFailure {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into() }
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody { error: ErrorDetail { code: self.code, message: self.message } }),
        ).into_response()
    }
}

fn constant_time_match(actual: &str, expected: &str) -> bool {
    let mut diff = actual.len() ^ expected.len();
    for (index, byte) in expected.bytes().enumerate() {
        diff |= (actual.as_bytes().get(index).copied().unwrap_or(0) ^ byte) as usize;
    }
    diff == 0
}

fn authorize(state: &ApiState, headers: &HeaderMap) -> Result<(), ApiFailure> {
    if let Some(expected) = state.token.as_deref() {
        let received = headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()).unwrap_or("");
        if !constant_time_match(received, &format!("Bearer {expected}")) {
            return Err(ApiFailure::new(StatusCode::UNAUTHORIZED, "unauthorized", "missing or invalid bearer token"));
        }
    }
    Ok(())
}

fn require_json(headers: &HeaderMap) -> Result<(), ApiFailure> {
    let ct = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    if !ct.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json") {
        return Err(ApiFailure::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type", "Content-Type must be application/json"));
    }
    Ok(())
}

fn read_body(body: Result<Bytes, BytesRejection>) -> Result<Bytes, ApiFailure> {
    body.map_err(|error| {
        let status = error.into_response().status();
        if status == StatusCode::PAYLOAD_TOO_LARGE {
            ApiFailure::new(status, "payload_too_large", "JSON body exceeds the configured size limit")
        } else {
            ApiFailure::new(StatusCode::BAD_REQUEST, "body_read_failed", "could not read request body")
        }
    })
}

fn parse_body(body: &[u8]) -> Result<JobRequest, ApiFailure> {
    serde_json::from_slice(body)
        .map_err(|error| ApiFailure::new(StatusCode::BAD_REQUEST, "invalid_json", error.to_string()))
}

fn validate_budget(state_max_ms: u64, request: &JobRequest) -> Result<(), ApiFailure> {
    if request.options.max_runtime_ms > state_max_ms {
        return Err(ApiFailure::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "runtime_limit_exceeded",
            format!("requested max_runtime_ms exceeds server limit of {state_max_ms}"),
        ));
    }
    Ok(())
}

fn try_permit(state: &ApiState) -> Result<OwnedSemaphorePermit, ApiFailure> {
    state.running.clone().try_acquire_owned().map_err(|_| {
        ApiFailure::new(StatusCode::SERVICE_UNAVAILABLE, "optimizer_busy", "all optimization workers are busy; retry later")
    })
}

async fn health() -> Json<Value> {
    Json(json!({"status":"ok", "service":"industrial-cut-optimizer", "version": env!("CARGO_PKG_VERSION")}))
}

async fn get_capabilities(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiFailure> {
    authorize(&state, &headers)?;
    let info = serde_json::to_value(capabilities()).map_err(|e| {
        ApiFailure::new(StatusCode::INTERNAL_SERVER_ERROR, "serialization_error", e.to_string())
    })?;
    Ok(Json(info))
}

async fn openapi_document(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiFailure> {
    authorize(&state, &headers)?;
    let source = include_str!("../../../docs/openapi-stage18.json");
    let document = serde_json::from_str(source).map_err(|_| {
        ApiFailure::new(StatusCode::INTERNAL_SERVER_ERROR, "openapi_unavailable", "invalid bundled API document")
    })?;
    Ok(Json(document))
}

async fn post_validate(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<Json<Value>, ApiFailure> {
    authorize(&state, &headers)?;
    require_json(&headers)?;
    let payload = read_body(body)?;
    let permit = try_permit(&state)?;
    let max_job_runtime_ms = state.max_job_runtime_ms;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit; // acquired for the entire blocking operation
        let request = parse_body(&payload)?;
        validate_budget(max_job_runtime_ms, &request)?;
        let receipt = validate_request(&request).map_err(|e| {
            ApiFailure::new(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed", e.to_string())
        })?;
        serde_json::to_value(receipt).map_err(|e| {
            ApiFailure::new(StatusCode::INTERNAL_SERVER_ERROR, "serialization_error", e.to_string())
        })
    }).await.map_err(|_| ApiFailure::new(StatusCode::INTERNAL_SERVER_ERROR, "worker_failed", "optimization worker terminated unexpectedly"))??;
    Ok(Json(result))
}

async fn post_job(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<Json<Value>, ApiFailure> {
    authorize(&state, &headers)?;
    require_json(&headers)?;
    let payload = read_body(body)?;
    let permit = try_permit(&state)?;
    let max_job_runtime_ms = state.max_job_runtime_ms;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit; // retained until work has actually ended
        let request = parse_body(&payload)?;
        validate_budget(max_job_runtime_ms, &request)?;
        let response = run_request(request).map_err(|e| {
            ApiFailure::new(StatusCode::UNPROCESSABLE_ENTITY, "optimization_failed", e.to_string())
        })?;
        serde_json::to_value(response).map_err(|e| {
            ApiFailure::new(StatusCode::INTERNAL_SERVER_ERROR, "serialization_error", e.to_string())
        })
    }).await.map_err(|_| ApiFailure::new(StatusCode::INTERNAL_SERVER_ERROR, "worker_failed", "optimization worker terminated unexpectedly"))??;
    Ok(Json(result))
}

/// Same contract as `/api/v1/jobs` restricted to `generate_cut_plans`, but the
/// body is the plans document itself (`success`, `message`, `records`).
async fn post_cut_plans(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<Json<Value>, ApiFailure> {
    authorize(&state, &headers)?;
    require_json(&headers)?;
    let payload = read_body(body)?;
    let permit = try_permit(&state)?;
    let max_job_runtime_ms = state.max_job_runtime_ms;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit; // retained until work has actually ended
        let request = parse_body(&payload)?;
        if !matches!(request.operation, JobOperation::GenerateCutPlans { .. }) {
            return Err(ApiFailure::new(StatusCode::UNPROCESSABLE_ENTITY, "unsupported_operation",
                "this endpoint only accepts operation.type = generate_cut_plans"));
        }
        validate_budget(max_job_runtime_ms, &request)?;
        let response = run_request(request).map_err(|e| {
            ApiFailure::new(StatusCode::UNPROCESSABLE_ENTITY, "optimization_failed", e.to_string())
        })?;
        match response.output {
            JobOutput::CutPlans { result } => Ok(result),
            _ => Err(ApiFailure::new(StatusCode::INTERNAL_SERVER_ERROR, "unexpected_output", "engine returned a different output")),
        }
    }).await.map_err(|_| ApiFailure::new(StatusCode::INTERNAL_SERVER_ERROR, "worker_failed", "optimization worker terminated unexpectedly"))??;
    Ok(Json(result))
}

/// Static, dependency-free cutting-plan viewer. It contains no data; jobs it
/// submits go through the authenticated endpoints.
async fn viewer() -> Html<&'static str> {
    Html(include_str!("../../../viewer/index.html"))
}

/// Build the API independently from network I/O; this is also used by HTTP tests.
/// Validate configuration using `config.validate()` before exposing a listener.
pub fn router(config: ApiConfig) -> Router {
    let max_bytes = config.max_body_bytes;
    let state = Arc::new(ApiState {
        token: config.api_key,
        running: Arc::new(Semaphore::new(config.max_parallel_jobs)),
        max_job_runtime_ms: config.max_job_runtime_ms,
    });
    Router::new()
        .route("/healthz", get(health))
        .route("/api/v1/capabilities", get(get_capabilities))
        .route("/api/v1/openapi.json", get(openapi_document))
        .route("/api/v1/jobs", post(post_job))
        .route("/api/v1/jobs/validate", post(post_validate))
        .route("/api/v1/cut-plans", post(post_cut_plans))
        .route("/viewer", get(viewer))
        .route("/", get(|| async { Redirect::temporary("/viewer") }))
        .layer(DefaultBodyLimit::max(max_bytes))
        .with_state(state)
}

/// Run an independent Rust HTTP server. TLS termination belongs in a reverse
/// proxy; Postman on localhost uses plain HTTP by default.
pub async fn serve(config: ApiConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    config.validate()?;
    let bind = config.bind;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("optimizer-api listening on http://{bind}");
    axum::serve(listener, router(config))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn request(path: &str, body: &str) -> Request<Body> {
        Request::builder().method("POST").uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned())).expect("request")
    }

    #[tokio::test]
    async fn health_check_is_public() {
        let response = router(ApiConfig::default()).oneshot(
            Request::builder().uri("/healthz").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn job_can_run_using_existing_contract() {
        let body = include_str!("../../../examples/system-check.json");
        let response = router(ApiConfig::default()).oneshot(request("/api/v1/jobs", body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["output"]["type"], "system_check");
        assert_eq!(value["output"]["core_ready"], true);
    }

    #[tokio::test]
    async fn validate_endpoint_accepts_request() {
        let body = include_str!("../../../examples/system-check.json");
        let response = router(ApiConfig::default()).oneshot(request("/api/v1/jobs/validate", body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn invalid_schema_is_422() {
        let body = r#"{"schema_version":"999","job_id":"test","operation":{"type":"system_check"}}"#;
        let response = router(ApiConfig::default()).oneshot(request("/api/v1/jobs", body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn malformed_json_is_400() {
        let response = router(ApiConfig::default()).oneshot(request("/api/v1/jobs", "{" )).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn an_api_key_restricts_jobs_but_not_health() {
        let config = ApiConfig { api_key: Some("at-least-sixteen-characters".into()), ..ApiConfig::default() };
        let body = include_str!("../../../examples/system-check.json");
        let response = router(config.clone()).oneshot(request("/api/v1/jobs", body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let authorized = Request::builder().method("POST").uri("/api/v1/jobs")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer at-least-sixteen-characters")
            .body(Body::from(body)).unwrap();
        let response = router(config).oneshot(authorized).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn oversized_json_returns_413() {
        let config = ApiConfig { max_body_bytes: 1024, ..ApiConfig::default() };
        let body = format!("{}{}", include_str!("../../../examples/system-check.json"), " ".repeat(4096));
        let response = router(config).oneshot(request("/api/v1/jobs", &body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn missing_content_type_is_415() {
        let response = router(ApiConfig::default()).oneshot(
            Request::builder().method("POST").uri("/api/v1/jobs")
                .body(Body::from("{}".to_owned())).unwrap()
        ).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn cut_plans_endpoint_returns_plans_document() {
        let mut job: Value = serde_json::from_str(include_str!("../../../examples/generate-cut-plans-rovere.json")).unwrap();
        for part in job["operation"]["model"]["parts"].as_array_mut().unwrap() {
            part["quantity"] = json!(1);
        }
        let response = router(ApiConfig::default())
            .oneshot(request("/api/v1/cut-plans", &job.to_string())).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["success"], true);
        assert_eq!(body["records"][0]["summary"]["pieces"], 12);
    }

    #[tokio::test]
    async fn cut_plans_endpoint_rejects_other_operations() {
        let body = include_str!("../../../examples/system-check.json");
        let response = router(ApiConfig::default()).oneshot(request("/api/v1/cut-plans", body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn viewer_is_served_as_html() {
        let response = router(ApiConfig::default()).oneshot(
            Request::builder().uri("/viewer").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&bytes).contains("Planos de corte"));
    }

    #[tokio::test]
    async fn capability_and_openapi_are_available() {
        let app = router(ApiConfig::default());
        for path in ["/api/v1/capabilities", "/api/v1/openapi.json"] {
            let response = app.clone().oneshot(Request::builder().uri(path).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "endpoint: {path}");
        }
    }

    #[test]
    fn capacity_is_bounded_to_prevent_overload() {
        let state = ApiState {
            token: None,
            running: Arc::new(Semaphore::new(1)),
            max_job_runtime_ms: 120_000,
        };
        let _first = try_permit(&state).unwrap_or_else(|_| panic!("first slot must be available"));
        let second = try_permit(&state).err().expect("second request must fail");
        assert_eq!(second.status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn server_runtime_budget_is_enforced() {
        let request: JobRequest = serde_json::from_str(
            r#"{"schema_version":"1.0","job_id":"test","options":{"max_runtime_ms":30000},"operation":{"type":"system_check"}}"#,
        ).unwrap();
        assert!(validate_budget(1000, &request).is_err());
        assert!(validate_budget(30000, &request).is_ok());
    }

    #[test]
    fn refuse_public_bind_without_authentication() {
        let config = ApiConfig { bind: "0.0.0.0:8080".parse().unwrap(), ..ApiConfig::default() };
        assert!(config.validate().is_err());
    }

    #[test]
    fn accept_public_bind_with_authentication() {
        let config = ApiConfig {
            bind: "0.0.0.0:8080".parse().unwrap(),
            api_key: Some("this-is-long-enough".into()),
            ..ApiConfig::default()
        };
        assert!(config.validate().is_ok());
    }
}
