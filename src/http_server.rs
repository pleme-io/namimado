//! Axum HTTP server — one face of the namimado control plane.
//!
//! Routes mirror the `openapi.yaml` spec exactly. Every handler is a
//! thin wrapper over [`crate::service::NamimadoService`], so behavior
//! stays byte-identical with the MCP surface and any generated SDK.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;
use std::net::SocketAddr;
use tracing::info;

use crate::api::{
    AddBookmarkRequest, ApiError, BookmarkInfo, HistoryInfo, NavigateRequest, NavigateResponse,
    ReloadResponse, ReportResponse, RulesInventory, StateCellValue, StatusResponse, StorageEntry,
    StorageSetRequest, StorageSummary,
};
use crate::service::NamimadoService;

/// Assemble the full router. Exposed as a function so tests can mount
/// the handlers into an in-process axum test server.
#[must_use]
pub fn router(service: NamimadoService) -> Router {
    Router::new()
        // API surface — mirrors openapi.yaml exactly.
        .route("/status", get(handle_status))
        .route("/navigate", post(handle_navigate))
        .route("/report", get(handle_report))
        .route("/state", get(handle_state))
        .route("/dom", get(handle_dom))
        .route("/rules", get(handle_rules))
        .route("/reload", post(handle_reload))
        .route("/typescape", get(handle_typescape))
        .route("/theme", get(handle_theme_json))
        .route("/theme.css", get(handle_theme_css))
        .route("/accessibility", get(handle_accessibility))
        .route("/history", get(handle_history))
        .route("/history", delete(handle_history_clear))
        .route("/bookmarks", get(handle_bookmarks_list))
        .route("/bookmarks", post(handle_bookmark_add))
        .route("/bookmarks", delete(handle_bookmark_remove))
        .route("/storage", get(handle_storage_list))
        .route(
            "/storage/:name",
            get(handle_storage_read)
                .post(handle_storage_set)
                .delete(handle_storage_delete),
        )
        .route("/openapi.yaml", get(handle_openapi_yaml))
        .route("/openapi.json", get(handle_openapi_json))
        // Inspector SPA — polls the API, shows substrate live.
        .route("/", get(|| async { Redirect::permanent("/ui") }))
        .route("/ui", get(handle_inspector))
        .with_state(service)
}

pub async fn serve(service: NamimadoService, addr: SocketAddr) -> anyhow::Result<()> {
    let app = router(service);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr().unwrap_or(addr);
    info!(%bound, "namimado HTTP server listening");
    eprintln!("  inspector UI   http://{bound}/ui");
    eprintln!("  openapi spec   http://{bound}/openapi.yaml");
    eprintln!("  navigate tool  curl -XPOST http://{bound}/navigate -d '{{\"url\":\"…\"}}'");
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

async fn handle_rules(State(svc): State<NamimadoService>) -> Json<RulesInventory> {
    Json(svc.rules_inventory())
}

async fn handle_typescape() -> Json<crate::typescape::NamimadoTypescape> {
    Json(crate::typescape::typescape())
}

async fn handle_theme_json() -> Json<serde_json::Value> {
    Json(serde_json::to_value(crate::theme::current_scheme()).unwrap_or_default())
}

async fn handle_theme_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        crate::theme::theme_css(),
    )
}

async fn handle_accessibility(
    State(svc): State<NamimadoService>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    svc.last_accessibility_tree()
        .map(Json)
        .ok_or_else(|| {
            ApiErrorResponse(
                StatusCode::NOT_FOUND,
                ApiError::new("no_navigate_yet")
                    .with_detail("call POST /navigate first"),
            )
        })
}

#[derive(Debug, Deserialize, Default)]
struct HistoryQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn handle_history(
    State(svc): State<NamimadoService>,
    Query(q): Query<HistoryQuery>,
) -> Json<Vec<HistoryInfo>> {
    if let Some(query) = q.q.as_deref().filter(|s| !s.is_empty()) {
        Json(svc.history_search(query))
    } else {
        Json(svc.history_recent(q.limit.unwrap_or(50)))
    }
}

async fn handle_history_clear(State(svc): State<NamimadoService>) -> StatusCode {
    svc.history_clear();
    StatusCode::NO_CONTENT
}

async fn handle_bookmarks_list(State(svc): State<NamimadoService>) -> Json<Vec<BookmarkInfo>> {
    Json(svc.bookmarks_list())
}

async fn handle_bookmark_add(
    State(svc): State<NamimadoService>,
    Json(req): Json<AddBookmarkRequest>,
) -> Result<Json<BookmarkAddResponse>, ApiErrorResponse> {
    svc.bookmark_add(req)
        .map(|added| Json(BookmarkAddResponse { added }))
        .map_err(|e| {
            ApiErrorResponse(
                StatusCode::BAD_REQUEST,
                ApiError::new("bookmark_add_failed").with_detail(e.to_string()),
            )
        })
}

#[derive(Debug, serde::Serialize)]
struct BookmarkAddResponse {
    added: bool,
}

#[derive(Debug, Deserialize)]
struct RemoveBookmarkQuery {
    url: String,
}

async fn handle_bookmark_remove(
    State(svc): State<NamimadoService>,
    Query(q): Query<RemoveBookmarkQuery>,
) -> Result<Json<BookmarkAddResponse>, ApiErrorResponse> {
    svc.bookmark_remove(&q.url)
        .map(|removed| Json(BookmarkAddResponse { added: removed }))
        .map_err(|e| {
            ApiErrorResponse(
                StatusCode::BAD_REQUEST,
                ApiError::new("bookmark_remove_failed").with_detail(e.to_string()),
            )
        })
}

#[derive(Debug, Deserialize, Default)]
struct StorageKeyQuery {
    #[serde(default)]
    key: Option<String>,
}

async fn handle_storage_list(State(svc): State<NamimadoService>) -> Json<Vec<StorageSummary>> {
    Json(svc.storage_list())
}

async fn handle_storage_read(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
    Query(q): Query<StorageKeyQuery>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    if let Some(key) = q.key.as_deref() {
        match svc.storage_get(&name, key) {
            Some(v) => Ok(Json(v)),
            None => Err(ApiErrorResponse(
                StatusCode::NOT_FOUND,
                ApiError::new("storage_key_missing")
                    .with_detail(format!("{name}:{key}")),
            )),
        }
    } else {
        match svc.storage_entries(&name) {
            Some(entries) => Ok(Json(serde_json::to_value(&entries).unwrap_or_default())),
            None => Err(ApiErrorResponse(
                StatusCode::NOT_FOUND,
                ApiError::new("storage_unknown").with_detail(name),
            )),
        }
    }
}

async fn handle_storage_set(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
    Json(req): Json<StorageSetRequest>,
) -> Result<Json<StorageEntry>, ApiErrorResponse> {
    let entry = StorageEntry {
        key: req.key.clone(),
        value: req.value.clone(),
    };
    if svc.storage_set(&name, req) {
        Ok(Json(entry))
    } else {
        Err(ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("storage_unknown").with_detail(name),
        ))
    }
}

async fn handle_storage_delete(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
    Query(q): Query<StorageKeyQuery>,
) -> Result<StatusCode, ApiErrorResponse> {
    let Some(key) = q.key else {
        return Err(ApiErrorResponse(
            StatusCode::BAD_REQUEST,
            ApiError::new("missing_key").with_detail("DELETE /storage/:name requires ?key="),
        ));
    };
    if svc.storage_delete(&name, &key) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("storage_key_missing")
                .with_detail(format!("{name}:{key}")),
        ))
    }
}

async fn handle_reload(
    State(svc): State<NamimadoService>,
) -> Result<Json<ReloadResponse>, ApiErrorResponse> {
    // SubstratePipeline::load() constructs reqwest::blocking client
    // which panics inside tokio. Spawn onto the blocking pool.
    tokio::task::spawn_blocking(move || svc.reload())
        .await
        .map(Json)
        .map_err(|e| {
            ApiErrorResponse(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiError::new("join_error").with_detail(e.to_string()),
            )
        })
}

async fn handle_dom(State(svc): State<NamimadoService>) -> Result<Response, ApiErrorResponse> {
    match svc.last_dom_sexp() {
        Some(sexp) => Ok((
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            sexp,
        )
            .into_response()),
        None => Err(ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("no_navigate_yet").with_detail("call POST /navigate first"),
        )),
    }
}

async fn handle_openapi_yaml() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/yaml")],
        include_str!("../openapi.yaml"),
    )
}

/// Substrate inspector SPA. Static HTML+JS that polls /state, /report,
/// /status every 2s and lets the user navigate by POSTing to /navigate.
/// Same HTTP surface any other client uses — dogfooded visibility.
async fn handle_inspector() -> Html<&'static str> {
    Html(include_str!("../assets/inspector.html"))
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
