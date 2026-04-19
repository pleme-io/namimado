//! Axum HTTP server — one face of the namimado control plane.
//!
//! Routes mirror the `openapi.yaml` spec exactly. Every handler is a
//! thin wrapper over [`crate::service::NamimadoService`], so behavior
//! stays byte-identical with the MCP surface and any generated SDK.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::net::SocketAddr;
use tracing::info;

use crate::api::{ApiError, NavigateRequest, NavigateResponse, ReportResponse, StateCellValue, StatusResponse};
use crate::service::NamimadoService;

/// Assemble the full router. Exposed as a function so tests can mount
/// the handlers into an in-process axum test server.
#[must_use]
pub fn router(service: NamimadoService) -> Router {
    Router::new()
        .route("/status", get(handle_status))
        .route("/navigate", post(handle_navigate))
        .route("/report", get(handle_report))
        .route("/state", get(handle_state))
        .route("/openapi.yaml", get(handle_openapi_yaml))
        .route("/openapi.json", get(handle_openapi_json))
        .with_state(service)
}

pub async fn serve(service: NamimadoService, addr: SocketAddr) -> anyhow::Result<()> {
    let app = router(service);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "namimado HTTP server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

// ─── handlers ────────────────────────────────────────────────────

async fn handle_status(State(svc): State<NamimadoService>) -> Json<StatusResponse> {
    Json(svc.status())
}

async fn handle_navigate(
    State(svc): State<NamimadoService>,
    Json(req): Json<NavigateRequest>,
) -> Result<Json<NavigateResponse>, ApiErrorResponse> {
    // NamimadoService::navigate is synchronous because nami-core uses
    // `reqwest::blocking`. Hop onto the blocking pool so we don't stall
    // the tokio reactor.
    tokio::task::spawn_blocking(move || svc.navigate(req))
        .await
        .map_err(|e| {
            ApiErrorResponse(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiError::new("join_error").with_detail(e.to_string()),
            )
        })?
        .map(Json)
        .map_err(|e| {
            ApiErrorResponse(
                StatusCode::BAD_REQUEST,
                ApiError::new("navigate_failed").with_detail(e.to_string()),
            )
        })
}

async fn handle_report(
    State(svc): State<NamimadoService>,
) -> Result<Json<ReportResponse>, ApiErrorResponse> {
    svc.last_report()
        .map(Json)
        .ok_or_else(|| ApiErrorResponse(StatusCode::NOT_FOUND, ApiError::new("no_navigate_yet").with_detail("call POST /navigate first")))
}

async fn handle_state(State(svc): State<NamimadoService>) -> Json<Vec<StateCellValue>> {
    Json(svc.state_snapshot())
}

async fn handle_openapi_yaml() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/yaml")],
        include_str!("../openapi.yaml"),
    )
}

async fn handle_openapi_json() -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let yaml = include_str!("../openapi.yaml");
    let value: serde_json::Value = serde_yaml::from_str(yaml).map_err(|e| {
        ApiErrorResponse(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::new("openapi_yaml_malformed").with_detail(e.to_string()),
        )
    })?;
    Ok(Json(value))
}

// ─── error shim ──────────────────────────────────────────────────

struct ApiErrorResponse(StatusCode, ApiError);

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}
