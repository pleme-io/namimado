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
    /// Unix epoch seconds when the substrate was last (re)loaded.
    pub loaded_at_epoch: u64,
    /// How many times /reload has been called. `0` = first load only.
    pub reload_count: u64,
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
    /// WASM agents that ran successfully on this navigate.
    pub wasm_agents_fired: usize,
    pub wasm_agent_hits: Vec<String>,
    /// Elements stripped by `(defblocker …)` rules this navigate.
    pub blocker_applied: usize,
    pub blocker_hits: Vec<String>,
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
            wasm_agents_fired: r.wasm_agents_fired,
            wasm_agent_hits: r.wasm_agent_hits.clone(),
            blocker_applied: r.blocker_applied,
            blocker_hits: r.blocker_hits.clone(),
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

/// POST /reload response — confirms the pipeline was rebuilt and
/// surfaces the fresh inventory so the caller doesn't need a second
/// round-trip to know what's now loaded.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReloadResponse {
    /// True when the reload actually re-ran the loader (always true
    /// with `browser-core`; false in the degraded no-browser-core
    /// build that has nothing to reload).
    pub reloaded: bool,
    /// How many times /reload has been called, including this one.
    pub reload_count: u64,
    /// The freshly loaded rule inventory.
    pub rules: RulesInventory,
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
    pub wasm_agents: Vec<String>,
    pub blockers: Vec<String>,
    pub storages: Vec<String>,
    pub extensions: Vec<String>,
    pub readers: Vec<String>,
    pub commands: Vec<String>,
    pub binds: Vec<String>,
    pub omniboxes: Vec<String>,
}

/// One entry in the browsing history. Timestamp is Unix seconds.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HistoryInfo {
    pub title: String,
    pub url: String,
    pub visited_at: i64,
    pub visit_count: u32,
}

impl HistoryInfo {
    #[must_use]
    pub fn from_entry(e: &crate::browser::history::HistoryEntry) -> Self {
        Self {
            title: e.title.clone(),
            url: e.url.to_string(),
            visited_at: e.timestamp,
            visit_count: e.visit_count,
        }
    }
}

/// One bookmark.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BookmarkInfo {
    pub title: String,
    pub url: String,
    pub folder: Option<String>,
    pub tags: Vec<String>,
    pub added_at: i64,
}

impl BookmarkInfo {
    #[must_use]
    pub fn from_bookmark(b: &crate::browser::bookmark::Bookmark) -> Self {
        Self {
            title: b.title.clone(),
            url: b.url.to_string(),
            folder: b.folder.clone(),
            tags: b.tags.clone(),
            added_at: b.created_at,
        }
    }
}

/// POST /storage/:name — input. Value is arbitrary JSON.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StorageSetRequest {
    pub key: String,
    pub value: serde_json::Value,
}

/// GET /storage — per-store summary.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StorageSummary {
    pub name: String,
    pub entry_count: usize,
}

/// GET /storage/:name — per-entry snapshot for one store.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StorageEntry {
    pub key: String,
    pub value: serde_json::Value,
}

/// POST /extensions/verify — verify a signed-extension envelope
/// against the namimado trust DB. Body: a full SignedExtension JSON
/// (spec + signature). Returns status + optional signer metadata.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VerifyExtensionResponse {
    /// "trusted" | "valid-but-untrusted" | "invalid"
    pub status: String,
    pub public_key: Option<String>,
    pub signed_by: Option<String>,
    pub detail: Option<String>,
}

/// POST /trustdb — add/remove body.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrustdbKeyRequest {
    pub public_key: String,
    #[serde(default)]
    pub signed_by: Option<String>,
}

/// GET /omnibox — one suggestion row.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OmniboxSuggestion {
    pub kind: String,
    pub label: String,
    pub detail: Option<String>,
    pub action: String,
    pub score: f32,
}

/// GET /omnibox — response envelope.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct OmniboxResponse {
    pub query: String,
    pub profile: String,
    pub suggestions: Vec<OmniboxSuggestion>,
}

/// POST /commands/dispatch — input. Simulates a key sequence against
/// the bind registry in a given mode. Useful for testing bindings
/// from MCP / tests without wiring into the GPU key pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DispatchKeyRequest {
    /// Typed-so-far sequence. Canonicalization is performed server-side.
    pub typed: String,
    /// Dispatch mode. Common values: "normal", "insert", "visual",
    /// "command", "any". Default "any".
    #[serde(default)]
    pub mode: Option<String>,
}

/// POST /commands/dispatch — response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DispatchKeyResponse {
    /// "run" | "prefix" | "miss".
    pub outcome: String,
    /// When outcome = "run": the resolved command name.
    pub command: Option<String>,
    /// When outcome = "run": the built-in action, if any.
    pub action: Option<String>,
    /// When outcome = "run": the tatara-lisp body, if any.
    pub body: Option<String>,
    /// When outcome = "run": the canonical key that fired.
    pub key: Option<String>,
}

/// GET /commands — one row per (defcommand) + its current bindings.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CommandInfo {
    pub name: String,
    pub description: Option<String>,
    pub action: Option<String>,
    pub body: Option<String>,
    pub default_key: Option<String>,
    /// All bound chords that currently target this command.
    pub bound_keys: Vec<String>,
}

/// GET /reader — simplified-view response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReaderResponse {
    pub spec_name: String,
    pub title: Option<String>,
    pub byline: Option<String>,
    /// Plain-text render of the simplified content.
    pub text: String,
    /// Simplified DOM serialized back to HTML.
    pub html: String,
    pub word_count: usize,
}

/// GET /extensions — one summary row per installed extension.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionSummary {
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub host_permissions_count: usize,
    pub rules_count: usize,
}

/// POST /extensions/:name/enabled — toggle body.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionToggleRequest {
    pub enabled: bool,
}

/// POST /extensions — install from raw Lisp source. Server compiles
/// the first (defextension …) form it finds; other def* forms in the
/// same source are installed into their respective registries too.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionInstallRequest {
    pub lisp_source: String,
}

/// POST /extensions response — content hash after install.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExtensionInstallResponse {
    pub installed: String,
    pub content_hash: String,
}

/// POST /bookmarks — input.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct AddBookmarkRequest {
    pub url: String,
    pub title: Option<String>,
    pub folder: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
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
                wasm_agents_fired: 2,
                wasm_agent_hits: vec!["scraper → 128 bytes (fuel=4000 ms=3)".into()],
                blocker_applied: 3,
                blocker_hits: vec!["trackers : .ad <div>".into()],
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        // Every field worth observing is in the JSON.
        assert!(json.contains("text_render"));
        assert!(json.contains("dom_sexp"));
        assert!(json.contains("inline_lisp_evaluated"));
        assert!(json.contains("normalize_applied"));
        assert!(json.contains("rule-a : div → n-card"));
        assert!(json.contains("wasm_agents_fired"));
        assert!(json.contains("128 bytes"));
        assert!(json.contains("blocker_applied"));
        assert!(json.contains("trackers : .ad <div>"));

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

    #[test]
    fn reload_response_serializes_rules_inventory() {
        let resp = ReloadResponse {
            reloaded: true,
            reload_count: 7,
            rules: RulesInventory {
                normalize_rules: vec!["a".into(), "b".into()],
                ..RulesInventory::default()
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"reload_count\":7"));
        assert!(json.contains("\"normalize_rules\":[\"a\",\"b\"]"));
        let back: ReloadResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.rules.normalize_rules.len(), 2);
    }

    #[test]
    fn status_response_has_reload_fields() {
        let s = StatusResponse {
            service: "namimado".into(),
            version: "0.1.0".into(),
            features: vec!["browser-core".into()],
            last_url: None,
            loaded_at_epoch: 1_700_000_000,
            reload_count: 3,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"loaded_at_epoch\":1700000000"));
        assert!(json.contains("\"reload_count\":3"));
    }
}
