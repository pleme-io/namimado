//! Public API types — the wire format shared by every interface
//! surface (HTTP, MCP, SDK). All types are `Serialize + Deserialize +
//! JsonSchema` so a single author pass produces OpenAPI schemas, MCP
//! tool schemas, and typed Rust clients from one source.
//!
//! The canonical spec lives at `openapi.yaml` at the repo root;
//! treat this module as its Rust-side mirror.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "browser-core")]
use crate::webview::substrate::NavigateOutcome;

/// GET /status — health + feature inventory.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatusResponse {
    /// Service name (always `"namimado"`).
    pub service: String,
    /// Crate version (`CARGO_PKG_VERSION`).
    pub version: String,
    /// Compile-time features that are live (`browser-core`,
    /// `gpu-chrome`, `http-server`).
    pub features: Vec<String>,
    /// URL of the most recent navigate, if any.
    pub last_url: Option<String>,
}

/// POST /navigate — input.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigateRequest {
    /// URL (or bare host — `example.com` → `https://example.com`).
    pub url: String,
}

/// POST /navigate — output.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigateResponse {
    pub final_url: String,
    pub fetched_bytes: usize,
    pub title: Option<String>,
    /// Post-transform plain-text render of the page body. Not layouted
    /// — just concatenated text nodes in document order. This is what
    /// the native GPU window shows in the left pane; the HTTP /ui
    /// panel also surfaces it.
    pub text_render: String,
    /// The post-transform DOM rendered as S-expressions. This is the
    /// page **absorbed into Lisp space** — suitable for further
    /// programmatic processing via tatara-lisp, or for inspection.
    /// Depth-capped server-side.
    pub dom_sexp: String,
    pub report: ReportResponse,
}

impl NavigateResponse {
    #[cfg(feature = "browser-core")]
    #[must_use]
    pub fn from_outcome(o: &NavigateOutcome) -> Self {
        Self {
            final_url: o.final_url.to_string(),
            fetched_bytes: o.fetched_bytes,
            title: o.title.clone(),
            text_render: o.text_render.clone(),
            dom_sexp: o.dom_sexp.clone(),
            report: ReportResponse::from_outcome(o),
        }
    }
}

/// GET /report + embedded in NavigateResponse — structured substrate
/// observables from one pass.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReportResponse {
    pub frameworks: Vec<FrameworkHit>,
    pub routes_matched: Option<String>,
    pub queries_dispatched: Vec<String>,
    pub effects_fired: usize,
    pub agents_fired: usize,
    pub transforms_applied: usize,
    pub transform_hits: Vec<String>,
    pub state_snapshot: Vec<StateCellValue>,
    pub derived_snapshot: Vec<StateCellValue>,
    /// Inline `<l-eval>` macros processed on this page.
    pub inline_lisp_evaluated: usize,
    pub inline_lisp_failed: usize,
}

impl ReportResponse {
    #[cfg(feature = "browser-core")]
    #[must_use]
    pub fn from_outcome(o: &NavigateOutcome) -> Self {
        let r = &o.report;
        Self {
            frameworks: r
                .frameworks
                .iter()
                .map(|(name, confidence)| FrameworkHit {
                    name: name.clone(),
                    confidence: *confidence,
                })
                .collect(),
            routes_matched: r.routes_matched.clone(),
            queries_dispatched: r.queries_dispatched.clone(),
            effects_fired: r.effects_fired,
            agents_fired: r.agents_fired,
            transforms_applied: r.transforms_applied,
            transform_hits: r.transform_hits.clone(),
            state_snapshot: r
                .state_snapshot
                .iter()
                .map(|(name, value)| StateCellValue {
                    name: name.clone(),
                    value: value.clone(),
                })
                .collect(),
            derived_snapshot: r
                .derived_snapshot
                .iter()
                .map(|(name, value)| StateCellValue {
                    name: name.clone(),
                    value: value.clone(),
                })
                .collect(),
            inline_lisp_evaluated: r.inline_lisp_evaluated,
            inline_lisp_failed: r.inline_lisp_failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FrameworkHit {
    pub name: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StateCellValue {
    pub name: String,
    /// Arbitrary JSON — matches the state store's value shape.
    pub value: Value,
}

/// Uniform error shape returned by every API surface.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ApiError {
    pub error: String,
    pub detail: Option<String>,
}

impl ApiError {
    #[must_use]
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            detail: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}
