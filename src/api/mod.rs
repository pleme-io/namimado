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
    /// `(defnormalize …)` rewrites applied. Each entry is
    /// `"rule-name : old-tag → new-tag"`.
    pub normalize_applied: usize,
    pub normalize_hits: Vec<String>,
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
            normalize_applied: r.normalize_applied,
            normalize_hits: r.normalize_hits.clone(),
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

/// Substrate rule inventory — what's loaded, by DSL keyword.
///
/// Useful for the inspector panel ("why didn't my rule fire?") and
/// for MCP agents browsing the authoring surface. Counts match the
/// startup log line; `names` gives quick identification.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct RulesInventory {
    pub states: Vec<String>,
    pub effects: Vec<String>,
    pub predicates: Vec<String>,
    pub plans: Vec<String>,
    pub agents: Vec<String>,
    pub routes: Vec<String>,
    pub queries: Vec<String>,
    pub derived: Vec<String>,
    pub components: Vec<String>,
    pub normalize_rules: Vec<String>,
    pub transforms: Vec<String>,
    pub aliases: Vec<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate_response_serializes_all_surfaces() {
        // The wire format must contain every field surfaced via HTTP,
        // MCP, and the inspector UI — no silent drops when a new field
        // is added to the domain type.
        let resp = NavigateResponse {
            final_url: "https://example.com/".into(),
            fetched_bytes: 512,
            title: Some("Example".into()),
            text_render: "some body".into(),
            dom_sexp: "(document)".into(),
            report: ReportResponse {
                frameworks: vec![FrameworkHit { name: "React".into(), confidence: 0.9 }],
                routes_matched: None,
                queries_dispatched: vec![],
                effects_fired: 2,
                agents_fired: 0,
                transforms_applied: 1,
                transform_hits: vec!["x".into()],
                state_snapshot: vec![],
                derived_snapshot: vec![],
                inline_lisp_evaluated: 3,
                inline_lisp_failed: 0,
                normalize_applied: 5,
                normalize_hits: vec!["rule-a : div → n-card".into()],
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        // Every field worth observing is in the JSON.
        assert!(json.contains("text_render"));
        assert!(json.contains("dom_sexp"));
        assert!(json.contains("inline_lisp_evaluated"));
        assert!(json.contains("normalize_applied"));
        assert!(json.contains("rule-a : div → n-card"));

        // Roundtrip.
        let back: NavigateResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.report.normalize_applied, 5);
        assert_eq!(back.report.inline_lisp_evaluated, 3);
    }

    #[test]
    fn api_error_with_detail_roundtrips() {
        let e = ApiError::new("bad_url").with_detail("scheme missing");
        let json = serde_json::to_string(&e).unwrap();
        let back: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(back.error, "bad_url");
        assert_eq!(back.detail.as_deref(), Some("scheme missing"));
    }
}
