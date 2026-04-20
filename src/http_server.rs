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
    AddBookmarkRequest, ApiError, BookmarkInfo, BoostInfo, BoostToggleRequest, CommandInfo,
    DispatchKeyRequest, DispatchKeyResponse, ExtensionInstallRequest, ExtensionInstallResponse,
    ExtensionSummary, ExtensionToggleRequest, FindRequest, FindResponse, GestureDispatchRequest,
    GestureDispatchResponse, HistoryInfo, I18nCoverage, I18nResponse, JsEvalRequest,
    JsEvalResponse, NavigateRequest, NavigateResponse, OmniboxResponse, PipResponse,
    ReaderResponse, ReloadResponse, ReportResponse, RulesInventory, SecurityPolicyResponse,
    RoutingResolveResponse, SessionTabInfo, SnapshotRecipeResponse, SpaceActivateResponse,
    SpaceActiveResponse,
    StateCellValue, StatusResponse, StorageEntry, StorageIndexSummary, StorageSetRequest,
    StorageSummary, TrustdbKeyRequest, VerifyExtensionResponse, ZoomResponse,
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
        .route("/storage/:name/index", get(handle_storage_index_list))
        .route(
            "/storage/:name/index/:path",
            get(handle_storage_by_index),
        )
        .route(
            "/storage/:name/index/:path/range",
            get(handle_storage_by_index_range),
        )
        .route("/i18n/:namespace", get(handle_i18n_get))
        .route("/i18n/:namespace/coverage", get(handle_i18n_coverage))
        .route("/security-policy", get(handle_security_policy))
        .route("/spoofs", get(handle_spoofs_list))
        .route("/spoof", get(handle_spoof_for))
        .route("/dns", get(handle_dns_list))
        .route("/dns/:name", get(handle_dns_get))
        .route("/routing", get(handle_routing_list))
        .route("/routing/resolve", get(handle_routing_resolve))
        .route("/spaces", get(handle_spaces_list))
        .route("/spaces/active", get(handle_space_active).delete(handle_space_deactivate))
        .route("/spaces/:name", get(handle_space_get))
        .route("/spaces/:name/activate", post(handle_space_activate))
        .route("/sidebars", get(handle_sidebars_list))
        .route("/splits", get(handle_splits_list))
        .route("/splits/:name", get(handle_split_get))
        .route("/js/eval", post(handle_js_eval))
        .route("/find", post(handle_find))
        .route("/zoom", get(handle_zoom))
        .route("/snapshot/recipe", get(handle_snapshot_recipe))
        .route("/pip", get(handle_pip))
        .route("/gesture/dispatch", post(handle_gesture_dispatch))
        .route("/boosts", get(handle_boosts_list))
        .route("/boosts/css", get(handle_boosts_css))
        .route("/boosts/:name/enabled", post(handle_boost_set_enabled))
        .route("/session/open", get(handle_session_open))
        .route("/session/closed", get(handle_session_closed))
        .route("/session/undo-close", post(handle_session_undo_close))
        .route("/reader", get(handle_reader))
        .route(
            "/extensions",
            get(handle_extensions_list).post(handle_extension_install),
        )
        .route(
            "/extensions/:name",
            get(handle_extension_get).delete(handle_extension_remove),
        )
        .route("/extensions/:name/enabled", post(handle_extension_set_enabled))
        .route("/extensions/verify", post(handle_extension_verify))
        .route("/trustdb", get(handle_trustdb_list).post(handle_trustdb_add))
        .route("/trustdb/:pubkey", delete(handle_trustdb_revoke))
        .route("/commands", get(handle_commands_list))
        .route("/commands/dispatch", post(handle_dispatch_key))
        .route("/omnibox", get(handle_omnibox))
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

#[derive(Debug, Deserialize, Default)]
struct ReaderQuery {
    #[serde(default)]
    name: Option<String>,
}

async fn handle_reader(
    State(svc): State<NamimadoService>,
    Query(q): Query<ReaderQuery>,
) -> Result<Json<ReaderResponse>, ApiErrorResponse> {
    svc.reader(q.name.as_deref()).map(Json).ok_or_else(|| {
        ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("reader_unavailable")
                .with_detail("no navigate yet, or no reader profile matched"),
        )
    })
}

async fn handle_extensions_list(
    State(svc): State<NamimadoService>,
) -> Json<Vec<ExtensionSummary>> {
    Json(svc.extensions_list())
}

async fn handle_extension_get(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    svc.extension_get(&name).map(Json).ok_or_else(|| {
        ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("extension_unknown").with_detail(name),
        )
    })
}

async fn handle_extension_install(
    State(svc): State<NamimadoService>,
    Json(req): Json<ExtensionInstallRequest>,
) -> Result<Json<ExtensionInstallResponse>, ApiErrorResponse> {
    svc.extension_install(req).map(Json).map_err(|e| {
        ApiErrorResponse(
            StatusCode::BAD_REQUEST,
            ApiError::new("extension_install_failed").with_detail(e.to_string()),
        )
    })
}

async fn handle_extension_remove(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiErrorResponse> {
    if svc.extension_remove(&name) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("extension_unknown").with_detail(name),
        ))
    }
}

async fn handle_extension_set_enabled(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
    Json(req): Json<ExtensionToggleRequest>,
) -> Result<Json<ExtensionSummary>, ApiErrorResponse> {
    if !svc.extension_set_enabled(&name, req) {
        return Err(ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("extension_unknown").with_detail(name.clone()),
        ));
    }
    svc.extensions_list()
        .into_iter()
        .find(|e| e.name == name)
        .map(Json)
        .ok_or_else(|| {
            ApiErrorResponse(
                StatusCode::NOT_FOUND,
                ApiError::new("extension_unknown").with_detail(name),
            )
        })
}

async fn handle_storage_index_list(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
) -> Result<Json<Vec<StorageIndexSummary>>, ApiErrorResponse> {
    svc.storage_index_summary(&name).map(Json).ok_or_else(|| {
        ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("storage_unknown").with_detail(name),
        )
    })
}

#[derive(Debug, Deserialize, Default)]
struct StorageByIndexQuery {
    value: String,
}

async fn handle_storage_by_index(
    State(svc): State<NamimadoService>,
    Path((name, path)): Path<(String, String)>,
    Query(q): Query<StorageByIndexQuery>,
) -> Result<Json<Vec<StorageEntry>>, ApiErrorResponse> {
    svc.storage_by_index(&name, &path, &q.value)
        .map(Json)
        .ok_or_else(|| {
            ApiErrorResponse(
                StatusCode::NOT_FOUND,
                ApiError::new("storage_or_index_unknown")
                    .with_detail(format!("{name}/{path}")),
            )
        })
}

async fn handle_spoofs_list(
    State(svc): State<NamimadoService>,
) -> Json<Vec<serde_json::Value>> {
    Json(svc.spoofs_list())
}

async fn handle_spoof_for(
    State(svc): State<NamimadoService>,
    Query(q): Query<HostQuery>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    svc.spoof_for(&q.host.unwrap_or_default())
        .map(Json)
        .ok_or_else(|| {
            ApiErrorResponse(
                StatusCode::NOT_FOUND,
                ApiError::new("no_spoof_matches"),
            )
        })
}

async fn handle_dns_list(
    State(svc): State<NamimadoService>,
) -> Json<Vec<serde_json::Value>> {
    Json(svc.dns_list())
}

async fn handle_dns_get(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    svc.dns_get(&name).map(Json).ok_or_else(|| {
        ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("dns_unknown").with_detail(name),
        )
    })
}

async fn handle_routing_list(
    State(svc): State<NamimadoService>,
) -> Json<Vec<serde_json::Value>> {
    Json(svc.routing_list())
}

async fn handle_routing_resolve(
    State(svc): State<NamimadoService>,
    Query(q): Query<HostQuery>,
) -> Json<RoutingResolveResponse> {
    Json(svc.routing_resolve(&q.host.unwrap_or_default()))
}

async fn handle_spaces_list(
    State(svc): State<NamimadoService>,
) -> Json<Vec<serde_json::Value>> {
    Json(svc.spaces_list())
}

async fn handle_space_get(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    svc.space_get(&name).map(Json).ok_or_else(|| {
        ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("space_unknown").with_detail(name),
        )
    })
}

async fn handle_space_activate(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
) -> Result<Json<SpaceActivateResponse>, ApiErrorResponse> {
    svc.space_activate(&name).map(Json).ok_or_else(|| {
        ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("space_unknown").with_detail(name),
        )
    })
}

async fn handle_space_active(State(svc): State<NamimadoService>) -> Json<SpaceActiveResponse> {
    Json(svc.space_active())
}

async fn handle_space_deactivate(State(svc): State<NamimadoService>) -> StatusCode {
    svc.space_deactivate();
    StatusCode::NO_CONTENT
}

#[derive(Debug, Deserialize, Default)]
struct SidebarsQuery {
    #[serde(default)]
    host: Option<String>,
}

async fn handle_sidebars_list(
    State(svc): State<NamimadoService>,
    Query(q): Query<SidebarsQuery>,
) -> Json<Vec<serde_json::Value>> {
    Json(svc.sidebars_list(q.host.as_deref()))
}

async fn handle_splits_list(
    State(svc): State<NamimadoService>,
) -> Json<Vec<serde_json::Value>> {
    Json(svc.splits_list())
}

async fn handle_split_get(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    svc.split_get(&name).map(Json).ok_or_else(|| {
        ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("split_unknown").with_detail(name),
        )
    })
}

async fn handle_js_eval(
    State(svc): State<NamimadoService>,
    Json(req): Json<JsEvalRequest>,
) -> Json<JsEvalResponse> {
    Json(svc.js_eval(req))
}

async fn handle_find(
    State(svc): State<NamimadoService>,
    Json(req): Json<FindRequest>,
) -> Result<Json<FindResponse>, ApiErrorResponse> {
    svc.find(req).map(Json).ok_or_else(|| {
        ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("no_navigate_yet").with_detail("call POST /navigate first"),
        )
    })
}

#[derive(Debug, Deserialize, Default)]
struct HostQuery {
    #[serde(default)]
    host: Option<String>,
}

async fn handle_zoom(
    State(svc): State<NamimadoService>,
    Query(q): Query<HostQuery>,
) -> Json<ZoomResponse> {
    Json(svc.zoom_for(&q.host.unwrap_or_default()))
}

#[derive(Debug, Deserialize, Default)]
struct SnapshotRecipeQuery {
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

async fn handle_snapshot_recipe(
    State(svc): State<NamimadoService>,
    Query(q): Query<SnapshotRecipeQuery>,
) -> Result<Json<SnapshotRecipeResponse>, ApiErrorResponse> {
    svc.snapshot_recipe(q.name.as_deref(), &q.host.unwrap_or_default())
        .map(Json)
        .ok_or_else(|| {
            ApiErrorResponse(
                StatusCode::NOT_FOUND,
                ApiError::new("no_recipe_matches"),
            )
        })
}

async fn handle_pip(
    State(svc): State<NamimadoService>,
    Query(q): Query<HostQuery>,
) -> Json<PipResponse> {
    Json(svc.pip_for(&q.host.unwrap_or_default()))
}

async fn handle_gesture_dispatch(
    State(svc): State<NamimadoService>,
    Json(req): Json<GestureDispatchRequest>,
) -> Json<GestureDispatchResponse> {
    Json(svc.gesture_dispatch(req))
}

async fn handle_boosts_list(
    State(svc): State<NamimadoService>,
    Query(q): Query<HostQuery>,
) -> Json<Vec<BoostInfo>> {
    Json(svc.boosts_list(q.host.as_deref()))
}

async fn handle_boosts_css(
    State(svc): State<NamimadoService>,
    Query(q): Query<HostQuery>,
) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        svc.boost_css(&q.host.unwrap_or_default()),
    )
}

async fn handle_boost_set_enabled(
    State(svc): State<NamimadoService>,
    Path(name): Path<String>,
    Json(req): Json<BoostToggleRequest>,
) -> Result<StatusCode, ApiErrorResponse> {
    if svc.boost_set_enabled(&name, req) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("boost_unknown").with_detail(name),
        ))
    }
}

async fn handle_session_open(State(svc): State<NamimadoService>) -> Json<Vec<SessionTabInfo>> {
    Json(svc.session_open())
}

async fn handle_session_closed(State(svc): State<NamimadoService>) -> Json<Vec<SessionTabInfo>> {
    Json(svc.session_closed())
}

async fn handle_session_undo_close(
    State(svc): State<NamimadoService>,
) -> Result<Json<SessionTabInfo>, ApiErrorResponse> {
    svc.session_undo_close().map(Json).ok_or_else(|| {
        ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("no_closed_tabs"),
        )
    })
}

#[derive(Debug, Deserialize, Default)]
struct IndexRangeQuery {
    #[serde(default)]
    lo: Option<String>,
    #[serde(default)]
    hi: Option<String>,
}

async fn handle_storage_by_index_range(
    State(svc): State<NamimadoService>,
    Path((name, path)): Path<(String, String)>,
    Query(q): Query<IndexRangeQuery>,
) -> Result<Json<Vec<StorageEntry>>, ApiErrorResponse> {
    let lo = q.lo.unwrap_or_default();
    let hi = q.hi.unwrap_or_else(|| "\u{10FFFF}".into());
    svc.storage_by_index_range(&name, &path, &lo, &hi)
        .map(Json)
        .ok_or_else(|| {
            ApiErrorResponse(
                StatusCode::NOT_FOUND,
                ApiError::new("storage_or_index_unknown")
                    .with_detail(format!("{name}/{path}")),
            )
        })
}

#[derive(Debug, Deserialize, Default)]
struct I18nQuery {
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    key: Option<String>,
}

async fn handle_i18n_get(
    State(svc): State<NamimadoService>,
    Path(namespace): Path<String>,
    Query(q): Query<I18nQuery>,
) -> Result<Json<I18nResponse>, ApiErrorResponse> {
    let Some(key) = q.key else {
        return Err(ApiErrorResponse(
            StatusCode::BAD_REQUEST,
            ApiError::new("missing_key").with_detail("GET /i18n/:ns requires ?key="),
        ));
    };
    let locale = q.locale.unwrap_or_else(|| "en".into());
    Ok(Json(svc.i18n_get(&namespace, &locale, &key)))
}

#[derive(Debug, Deserialize, Default)]
struct I18nCoverageQuery {
    #[serde(default)]
    locale: Option<String>,
}

async fn handle_i18n_coverage(
    State(svc): State<NamimadoService>,
    Path(namespace): Path<String>,
    Query(q): Query<I18nCoverageQuery>,
) -> Json<I18nCoverage> {
    let locale = q.locale.unwrap_or_else(|| "en".into());
    Json(svc.i18n_coverage(&namespace, &locale))
}

#[derive(Debug, Deserialize, Default)]
struct SecurityPolicyQuery {
    #[serde(default)]
    host: Option<String>,
}

async fn handle_security_policy(
    State(svc): State<NamimadoService>,
    Query(q): Query<SecurityPolicyQuery>,
) -> Json<SecurityPolicyResponse> {
    let host = q.host.unwrap_or_default();
    Json(svc.security_policy_for(&host))
}

async fn handle_extension_verify(
    State(svc): State<NamimadoService>,
    Json(signed): Json<nami_core::extension::SignedExtension>,
) -> Json<VerifyExtensionResponse> {
    Json(svc.verify_signed_extension(&signed))
}

async fn handle_trustdb_list(State(svc): State<NamimadoService>) -> Json<Vec<String>> {
    Json(svc.trustdb_keys())
}

async fn handle_trustdb_add(
    State(svc): State<NamimadoService>,
    Json(req): Json<TrustdbKeyRequest>,
) -> Result<Json<TrustdbKeyRequest>, ApiErrorResponse> {
    let pubkey = req.public_key.clone();
    let by = req.signed_by.clone();
    if svc.trust_pubkey(req) {
        Ok(Json(TrustdbKeyRequest {
            public_key: pubkey,
            signed_by: by,
        }))
    } else {
        Err(ApiErrorResponse(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::new("trustdb_locked"),
        ))
    }
}

async fn handle_trustdb_revoke(
    State(svc): State<NamimadoService>,
    Path(pubkey): Path<String>,
) -> Result<StatusCode, ApiErrorResponse> {
    if svc.revoke_pubkey(&pubkey) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiErrorResponse(
            StatusCode::NOT_FOUND,
            ApiError::new("trustdb_key_missing").with_detail(pubkey),
        ))
    }
}

#[derive(Debug, Deserialize, Default)]
struct OmniboxQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    profile: Option<String>,
}

async fn handle_omnibox(
    State(svc): State<NamimadoService>,
    Query(q): Query<OmniboxQuery>,
) -> Json<OmniboxResponse> {
    let query = q.q.unwrap_or_default();
    Json(svc.omnibox(&query, q.profile.as_deref()))
}

async fn handle_commands_list(State(svc): State<NamimadoService>) -> Json<Vec<CommandInfo>> {
    Json(svc.commands_list())
}

async fn handle_dispatch_key(
    State(svc): State<NamimadoService>,
    Json(req): Json<DispatchKeyRequest>,
) -> Json<DispatchKeyResponse> {
    Json(svc.dispatch_key(req))
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
