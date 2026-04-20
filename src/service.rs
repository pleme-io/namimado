//! Namimado control plane — **one service, many faces.**
//!
//! Every interface surface (MCP, HTTP, local CLI) delegates into
//! [`NamimadoService`]. The service owns the substrate pipeline, a
//! snapshot of the last navigate, and any shared state that needs to
//! survive across requests. It never opens a window and never talks to
//! GPU — it's the headless core.
//!
//! ## Why one service
//!
//! pleme-io's platform convention: author one OpenAPI spec, render
//! multiple surfaces (HTTP server, MCP server, SDK) from it. A shared
//! service struct gives every surface the same semantics — the MCP
//! "navigate" tool and the HTTP `POST /navigate` endpoint produce
//! byte-identical reports because they call the same method.

use anyhow::Result;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use url::Url;

use crate::api::{
    AddBookmarkRequest, BookmarkInfo, BoostInfo, BoostToggleRequest, CommandInfo,
    DispatchKeyRequest, DispatchKeyResponse, ExtensionInstallRequest, ExtensionInstallResponse,
    ExtensionSummary, ExtensionToggleRequest, FindMatchInfo, FindRequest, FindResponse,
    GestureDispatchRequest, GestureDispatchResponse, HistoryInfo, I18nCoverage, I18nResponse,
    JsEvalRequest, JsEvalResponse, NavigateRequest, NavigateResponse, OmniboxResponse,
    OmniboxSuggestion, PipResponse, ReaderResponse, ReloadResponse, ReportResponse,
    RulesInventory, SecurityPolicyResponse, SessionTabInfo, SnapshotRecipeResponse,
    ChatAskRequest, LlmCompletionRequest, LlmMessageDto, LlmResponseDto, OutlineRequest,
    RedirectRequest, RoutingResolveResponse, SpaceActivateResponse, SummarizeRequest,
    UrlCleanRequest, UrlRewriteResponse, SpaceActiveResponse, StateCellValue, StatusResponse, StorageEntry,
    StorageSetRequest, StorageSummary, TrustdbKeyRequest, VerifyExtensionResponse, ZoomResponse,
};
use crate::browser::bookmark::{Bookmark, BookmarkManager};
use crate::browser::history::HistoryManager;

#[cfg(feature = "browser-core")]
use crate::webview::substrate::{NavigateOutcome, SubstratePipeline};

/// Shared handle — cheap to clone; all clones see the same state.
#[derive(Clone)]
pub struct NamimadoService {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    #[cfg(feature = "browser-core")]
    pipeline: SubstratePipeline,
    #[cfg(feature = "browser-core")]
    last_outcome: Option<NavigateOutcome>,
    version: &'static str,
    loaded_at: SystemTime,
    reload_count: u64,
    history: HistoryManager,
    bookmarks: BookmarkManager,
}

impl NamimadoService {
    #[cfg(feature = "browser-core")]
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                pipeline: SubstratePipeline::load(),
                last_outcome: None,
                version: env!("CARGO_PKG_VERSION"),
                loaded_at: SystemTime::now(),
                reload_count: 0,
                history: HistoryManager::new(),
                bookmarks: BookmarkManager::new(),
            })),
        }
    }

    #[cfg(not(feature = "browser-core"))]
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                version: env!("CARGO_PKG_VERSION"),
                loaded_at: SystemTime::now(),
                reload_count: 0,
                history: HistoryManager::new(),
                bookmarks: BookmarkManager::new(),
            })),
        }
    }

    /// POST /reload — re-scan `substrate.d/*.lisp` + extensions.lisp +
    /// transforms.lisp + aliases.lisp and swap in a fresh pipeline.
    /// In-flight navigates complete first (mutex ordering). State
    /// store is reset too — seeded fresh from the new (defstate) specs.
    ///
    /// Also drops the cached theme scheme — the next call to
    /// `theme::current_scheme()` re-reads config, re-loads any
    /// scheme file, re-derives. That's how "one edit reflects
    /// across every surface" works.
    pub fn reload(&self) -> ReloadResponse {
        crate::theme::reload();
        #[cfg(feature = "browser-core")]
        {
            let fresh = SubstratePipeline::load();
            let inv_after = fresh.rules_inventory();
            let mut inner = self.inner.lock().expect("service mutex poisoned");
            inner.pipeline = fresh;
            inner.last_outcome = None;
            inner.loaded_at = SystemTime::now();
            inner.reload_count += 1;
            return ReloadResponse {
                reloaded: true,
                reload_count: inner.reload_count,
                rules: inv_after,
            };
        }

        #[cfg(not(feature = "browser-core"))]
        {
            let mut inner = self.inner.lock().expect("service mutex poisoned");
            inner.reload_count += 1;
            inner.loaded_at = SystemTime::now();
            ReloadResponse {
                reloaded: false,
                reload_count: inner.reload_count,
                rules: RulesInventory::default(),
            }
        }
    }

    /// GET /status — server liveness + feature set.
    pub fn status(&self) -> StatusResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let loaded_at_epoch = inner
            .loaded_at
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        StatusResponse {
            service: "namimado".to_owned(),
            version: inner.version.to_owned(),
            features: compile_features(),
            last_url: self.last_url(&inner),
            loaded_at_epoch,
            reload_count: inner.reload_count,
        }
    }

    /// POST /navigate — run the full nami-core substrate pipeline against
    /// a URL. Returns the structured report.
    pub fn navigate(&self, req: NavigateRequest) -> Result<NavigateResponse> {
        #[cfg(feature = "browser-core")]
        {
            let url = Url::parse(&req.url)
                .or_else(|_| Url::parse(&format!("https://{}", req.url)))?;
            let mut inner = self.inner.lock().expect("service mutex poisoned");
            let outcome = inner
                .pipeline
                .navigate(&url)
                .map_err(|e| anyhow::anyhow!(e))?;
            // Auto-record history — every successful navigate becomes
            // a substrate-visible event. Title comes from the rendered
            // page when available, URL-fallback otherwise.
            let title = outcome
                .title
                .clone()
                .unwrap_or_else(|| outcome.final_url.to_string());
            inner.history.record_visit(title, outcome.final_url.clone());
            let response = NavigateResponse::from_outcome(&outcome);
            inner.last_outcome = Some(outcome);
            Ok(response)
        }

        #[cfg(not(feature = "browser-core"))]
        {
            let _ = req;
            anyhow::bail!("browser-core feature disabled — rebuild with --features browser-core")
        }
    }

    /// GET /history — most recent visits, newest first.
    pub fn history_recent(&self, count: usize) -> Vec<HistoryInfo> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .history
            .recent(count)
            .iter()
            .map(HistoryInfo::from_entry)
            .collect()
    }

    /// GET /history?q= — search history by title or URL substring.
    pub fn history_search(&self, query: &str) -> Vec<HistoryInfo> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .history
            .search(query)
            .iter()
            .map(|e| HistoryInfo::from_entry(*e))
            .collect()
    }

    /// DELETE /history — wipe all.
    pub fn history_clear(&self) {
        let mut inner = self.inner.lock().expect("service mutex poisoned");
        inner.history.clear();
    }

    /// GET /storage — per-store summary.
    #[cfg(feature = "browser-core")]
    pub fn storage_list(&self) -> Vec<StorageSummary> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .storage_summary()
            .into_iter()
            .map(|(name, entry_count)| StorageSummary { name, entry_count })
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn storage_list(&self) -> Vec<StorageSummary> {
        Vec::new()
    }

    /// GET /storage/:name — full entry snapshot, single store.
    #[cfg(feature = "browser-core")]
    pub fn storage_entries(&self, store: &str) -> Option<Vec<StorageEntry>> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let s = inner.pipeline.get_store(store)?;
        Some(
            s.entries()
                .into_iter()
                .map(|(key, value)| StorageEntry { key, value })
                .collect(),
        )
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn storage_entries(&self, _store: &str) -> Option<Vec<StorageEntry>> {
        None
    }

    /// GET /storage/:name?key=K — single-value lookup.
    #[cfg(feature = "browser-core")]
    pub fn storage_get(&self, store: &str, key: &str) -> Option<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.get_store(store)?.get(key)
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn storage_get(&self, _s: &str, _k: &str) -> Option<serde_json::Value> {
        None
    }

    /// POST /storage/:name — set one key → value.
    #[cfg(feature = "browser-core")]
    pub fn storage_set(&self, store: &str, req: StorageSetRequest) -> bool {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let Some(s) = inner.pipeline.get_store(store) else {
            return false;
        };
        s.set(req.key, req.value);
        true
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn storage_set(&self, _s: &str, _r: StorageSetRequest) -> bool {
        false
    }

    /// DELETE /storage/:name?key=K — remove one key.
    #[cfg(feature = "browser-core")]
    pub fn storage_delete(&self, store: &str, key: &str) -> bool {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let Some(s) = inner.pipeline.get_store(store) else {
            return false;
        };
        s.delete(key)
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn storage_delete(&self, _s: &str, _k: &str) -> bool {
        false
    }

    /// GET /reader — apply a (defreader) profile to the last-navigated
    /// page. When `name` is None, uses the first matching profile for
    /// the page's host. Returns None when no navigate has happened.
    #[cfg(feature = "browser-core")]
    pub fn reader(&self, name: Option<&str>) -> Option<ReaderResponse> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let outcome = inner.last_outcome.as_ref()?;
        let sexp = outcome.dom_sexp.clone();
        let host = outcome.final_url.host_str().unwrap_or("").to_owned();
        drop(inner);

        let doc = nami_core::lisp::sexp_to_dom(&sexp).ok()?;
        let lock = self.inner.lock().expect("service mutex poisoned");
        let out = lock.pipeline.apply_reader(&doc, name, &host)?;
        let html = out.content.root.to_html();
        Some(ReaderResponse {
            spec_name: out.spec_name,
            title: out.title,
            byline: out.byline,
            text: out.text,
            html,
            word_count: out.word_count,
        })
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn reader(&self, _name: Option<&str>) -> Option<ReaderResponse> {
        None
    }

    /// GET /extensions — installed extension summary.
    #[cfg(feature = "browser-core")]
    pub fn extensions_list(&self) -> Vec<ExtensionSummary> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .extension_summary()
            .into_iter()
            .map(|(name, version, enabled, hosts, rules)| ExtensionSummary {
                name,
                version,
                enabled,
                host_permissions_count: hosts,
                rules_count: rules,
            })
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn extensions_list(&self) -> Vec<ExtensionSummary> {
        Vec::new()
    }

    /// GET /extensions/:name — full ExtensionSpec.
    #[cfg(feature = "browser-core")]
    pub fn extension_get(&self, name: &str) -> Option<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let spec = inner.pipeline.extension_get(name)?;
        serde_json::to_value(&spec).ok()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn extension_get(&self, _n: &str) -> Option<serde_json::Value> {
        None
    }

    /// POST /extensions/:name/enabled — toggle runtime enabled state.
    #[cfg(feature = "browser-core")]
    pub fn extension_set_enabled(&self, name: &str, req: ExtensionToggleRequest) -> bool {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.extension_set_enabled(name, req.enabled)
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn extension_set_enabled(&self, _n: &str, _r: ExtensionToggleRequest) -> bool {
        false
    }

    /// POST /extensions — install from Lisp source.
    #[cfg(feature = "browser-core")]
    pub fn extension_install(&self, req: ExtensionInstallRequest) -> Result<ExtensionInstallResponse> {
        let specs = nami_core::extension::compile(&req.lisp_source)
            .map_err(|e| anyhow::anyhow!("compile failed: {e}"))?;
        let first = specs
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("no (defextension …) form in source"))?;
        let installed_name = first.name.clone();
        let inner = self.inner.lock().expect("service mutex poisoned");
        let hash = inner
            .pipeline
            .extension_install(first)
            .ok_or_else(|| anyhow::anyhow!("registry lock poisoned"))?;
        Ok(ExtensionInstallResponse {
            installed: installed_name,
            content_hash: hash,
        })
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn extension_install(&self, _r: ExtensionInstallRequest) -> Result<ExtensionInstallResponse> {
        anyhow::bail!("browser-core feature disabled")
    }

    /// DELETE /extensions/:name — uninstall.
    #[cfg(feature = "browser-core")]
    pub fn extension_remove(&self, name: &str) -> bool {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.extension_remove(name)
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn extension_remove(&self, _n: &str) -> bool {
        false
    }

    /// Content hash of the currently installed extension set.
    #[cfg(feature = "browser-core")]
    pub fn extensions_content_hash(&self) -> String {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.extensions_content_hash()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn extensions_content_hash(&self) -> String {
        String::new()
    }

    /// POST /extensions/verify — check signature + trust status.
    #[cfg(feature = "browser-core")]
    pub fn verify_signed_extension(
        &self,
        signed: &nami_core::extension::SignedExtension,
    ) -> VerifyExtensionResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        match inner.pipeline.verify_signed_extension(signed) {
            Ok(nami_core::extension::VerificationStatus::Trusted {
                public_key_b64,
                signed_by,
            }) => VerifyExtensionResponse {
                status: "trusted".into(),
                public_key: Some(public_key_b64),
                signed_by,
                detail: None,
            },
            Ok(nami_core::extension::VerificationStatus::ValidButUntrusted {
                public_key_b64,
            }) => VerifyExtensionResponse {
                status: "valid-but-untrusted".into(),
                public_key: Some(public_key_b64),
                signed_by: signed.signature.signed_by.clone(),
                detail: Some("signature verified but key is not in the trust DB".into()),
            },
            Ok(nami_core::extension::VerificationStatus::Invalid(reason)) => {
                VerifyExtensionResponse {
                    status: "invalid".into(),
                    public_key: None,
                    signed_by: None,
                    detail: Some(reason),
                }
            }
            Err(e) => VerifyExtensionResponse {
                status: "invalid".into(),
                public_key: None,
                signed_by: None,
                detail: Some(e.to_string()),
            },
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn verify_signed_extension(
        &self,
        _signed: &nami_core::extension::SignedExtension,
    ) -> VerifyExtensionResponse {
        VerifyExtensionResponse {
            status: "invalid".into(),
            public_key: None,
            signed_by: None,
            detail: Some("browser-core feature disabled".into()),
        }
    }

    /// GET /trustdb — list of base64-encoded pubkeys.
    #[cfg(feature = "browser-core")]
    pub fn trustdb_keys(&self) -> Vec<String> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.trustdb_keys()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn trustdb_keys(&self) -> Vec<String> {
        Vec::new()
    }

    /// POST /trustdb — add a pubkey to the trust DB.
    #[cfg(feature = "browser-core")]
    pub fn trust_pubkey(&self, req: TrustdbKeyRequest) -> bool {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.trust_pubkey(&req.public_key)
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn trust_pubkey(&self, _req: TrustdbKeyRequest) -> bool {
        false
    }

    /// DELETE /trustdb/:pubkey — revoke.
    #[cfg(feature = "browser-core")]
    pub fn revoke_pubkey(&self, pubkey: &str) -> bool {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.revoke_pubkey(pubkey)
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn revoke_pubkey(&self, _p: &str) -> bool {
        false
    }

    /// GET /omnibox?q=… — unified URL-bar autocomplete. Feeds history,
    /// bookmarks, and live command/bind state into the ranker.
    #[cfg(feature = "browser-core")]
    pub fn omnibox(&self, query: &str, profile: Option<&str>) -> OmniboxResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");

        // Harvest local sources.
        let history: Vec<nami_core::omnibox::HistoryItem> = inner
            .history
            .recent(500)
            .iter()
            .map(|e| nami_core::omnibox::HistoryItem {
                title: e.title.clone(),
                url: e.url.to_string(),
                visit_count: e.visit_count,
            })
            .collect();
        let bookmarks: Vec<nami_core::omnibox::BookmarkItem> = inner
            .bookmarks
            .list(None)
            .iter()
            .map(|b| nami_core::omnibox::BookmarkItem {
                title: b.title.clone(),
                url: b.url.to_string(),
                tags: b.tags.clone(),
            })
            .collect();
        let tabs: Vec<nami_core::omnibox::TabItem> = Vec::new();
        let extensions: Vec<nami_core::omnibox::ExtensionItem> = inner
            .pipeline
            .extension_summary()
            .into_iter()
            .map(|(name, _v, enabled, _h, _r)| nami_core::omnibox::ExtensionItem {
                name,
                description: None,
                enabled,
            })
            .collect();

        let suggestions = inner
            .pipeline
            .omnibox_rank(query, profile, &history, &bookmarks, &tabs, &extensions);

        let profile_name = profile
            .map(str::to_owned)
            .or_else(|| inner.pipeline.omnibox_names().first().cloned())
            .unwrap_or_else(|| "default".to_owned());

        OmniboxResponse {
            query: query.to_owned(),
            profile: profile_name,
            suggestions: suggestions
                .into_iter()
                .map(|s| OmniboxSuggestion {
                    kind: match s.kind {
                        nami_core::omnibox::SuggestionKind::History => "history",
                        nami_core::omnibox::SuggestionKind::Bookmark => "bookmark",
                        nami_core::omnibox::SuggestionKind::Command => "command",
                        nami_core::omnibox::SuggestionKind::Tab => "tab",
                        nami_core::omnibox::SuggestionKind::Extension => "extension",
                        nami_core::omnibox::SuggestionKind::Search => "search",
                        nami_core::omnibox::SuggestionKind::Navigate => "navigate",
                    }
                    .to_owned(),
                    label: s.label,
                    detail: s.detail,
                    action: s.action,
                    score: s.score,
                })
                .collect(),
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn omnibox(&self, query: &str, profile: Option<&str>) -> OmniboxResponse {
        OmniboxResponse {
            query: query.to_owned(),
            profile: profile.unwrap_or("default").to_owned(),
            suggestions: Vec::new(),
        }
    }

    // ── AI pack ──────────────────────────────────────────────────

    #[cfg(feature = "browser-core")]
    pub fn llm_provider_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .llm_provider_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn summarize_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .summarize_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn chat_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .chat_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn llm_completion_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .llm_completion_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn summarize_run(&self, req: SummarizeRequest) -> LlmResponseDto {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let engine = inner.pipeline.llm_engine_name().to_owned();
        match inner.pipeline.summarize_run(&req.profile, &req.source) {
            Ok(r) => pack_llm_response(r, engine),
            Err(e) => pack_llm_error(e, engine),
        }
    }

    #[cfg(feature = "browser-core")]
    pub fn chat_ask(&self, req: ChatAskRequest) -> LlmResponseDto {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let engine = inner.pipeline.llm_engine_name().to_owned();
        let history: Vec<nami_core::llm::LlmMessage> = req
            .history
            .iter()
            .map(|m| nami_core::llm::LlmMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();
        match inner.pipeline.chat_ask(
            &req.profile,
            req.page_context.as_deref(),
            &history,
            &req.question,
        ) {
            Ok(r) => pack_llm_response(r, engine),
            Err(e) => pack_llm_error(e, engine),
        }
    }

    #[cfg(feature = "browser-core")]
    pub fn llm_completion_run(&self, req: LlmCompletionRequest) -> LlmResponseDto {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let engine = inner.pipeline.llm_engine_name().to_owned();
        match inner.pipeline.llm_completion_run(&req.profile, &req.prefix) {
            Ok(r) => pack_llm_response(r, engine),
            Err(e) => pack_llm_error(e, engine),
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn llm_provider_list(&self) -> Vec<serde_json::Value> { Vec::new() }
    #[cfg(not(feature = "browser-core"))]
    pub fn summarize_list(&self) -> Vec<serde_json::Value> { Vec::new() }
    #[cfg(not(feature = "browser-core"))]
    pub fn chat_list(&self) -> Vec<serde_json::Value> { Vec::new() }
    #[cfg(not(feature = "browser-core"))]
    pub fn llm_completion_list(&self) -> Vec<serde_json::Value> { Vec::new() }
    #[cfg(not(feature = "browser-core"))]
    pub fn summarize_run(&self, _r: SummarizeRequest) -> LlmResponseDto { disabled_response() }
    #[cfg(not(feature = "browser-core"))]
    pub fn chat_ask(&self, _r: ChatAskRequest) -> LlmResponseDto { disabled_response() }
    #[cfg(not(feature = "browser-core"))]
    pub fn llm_completion_run(&self, _r: LlmCompletionRequest) -> LlmResponseDto { disabled_response() }

    // ── Credentials pack ─────────────────────────────────────────

    #[cfg(feature = "browser-core")]
    pub fn autofill_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .autofill_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn password_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .password_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn passwords_for(&self, host: &str) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .passwords_for(host)
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn auth_saver_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .auth_saver_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn auth_saver_for(&self, host: &str) -> Option<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .auth_saver_for(host)
            .and_then(|s| serde_json::to_value(&s).ok())
    }

    #[cfg(feature = "browser-core")]
    pub fn secure_note_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .secure_note_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn passkey_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .passkey_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn passkeys_for(&self, rp_id: &str) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .passkeys_for(rp_id)
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn autofill_list(&self) -> Vec<serde_json::Value> { Vec::new() }
    #[cfg(not(feature = "browser-core"))]
    pub fn password_list(&self) -> Vec<serde_json::Value> { Vec::new() }
    #[cfg(not(feature = "browser-core"))]
    pub fn passwords_for(&self, _h: &str) -> Vec<serde_json::Value> { Vec::new() }
    #[cfg(not(feature = "browser-core"))]
    pub fn auth_saver_list(&self) -> Vec<serde_json::Value> { Vec::new() }
    #[cfg(not(feature = "browser-core"))]
    pub fn auth_saver_for(&self, _h: &str) -> Option<serde_json::Value> { None }
    #[cfg(not(feature = "browser-core"))]
    pub fn secure_note_list(&self) -> Vec<serde_json::Value> { Vec::new() }
    #[cfg(not(feature = "browser-core"))]
    pub fn passkey_list(&self) -> Vec<serde_json::Value> { Vec::new() }
    #[cfg(not(feature = "browser-core"))]
    pub fn passkeys_for(&self, _r: &str) -> Vec<serde_json::Value> { Vec::new() }

    // ── Mobile + download pack ───────────────────────────────────

    #[cfg(feature = "browser-core")]
    pub fn share_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .share_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn share_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    #[cfg(feature = "browser-core")]
    pub fn offline_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .offline_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn offline_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    #[cfg(feature = "browser-core")]
    pub fn pull_refresh_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .pull_refresh_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn pull_refresh_for(&self, host: &str) -> Option<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .pull_refresh_for(host)
            .and_then(|s| serde_json::to_value(&s).ok())
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn pull_refresh_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn pull_refresh_for(&self, _h: &str) -> Option<serde_json::Value> {
        None
    }

    #[cfg(feature = "browser-core")]
    pub fn download_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .download_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn download_get(&self, name: &str) -> Option<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .download_get(name)
            .and_then(|s| serde_json::to_value(&s).ok())
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn download_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn download_get(&self, _n: &str) -> Option<serde_json::Value> {
        None
    }

    // ── Reading pack ─────────────────────────────────────────────

    /// POST /outline — extract outline from the last-navigated page.
    #[cfg(feature = "browser-core")]
    pub fn outline_extract(&self, req: OutlineRequest) -> Option<Vec<serde_json::Value>> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let sexp = inner.last_outcome.as_ref()?.dom_sexp.clone();
        drop(inner);
        let doc = nami_core::lisp::sexp_to_dom(&sexp).ok()?;
        let lock = self.inner.lock().expect("service mutex poisoned");
        let entries = lock.pipeline.outline_extract(&doc, req.profile.as_deref());
        Some(
            entries
                .into_iter()
                .filter_map(|e| serde_json::to_value(&e).ok())
                .collect(),
        )
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn outline_extract(&self, _r: OutlineRequest) -> Option<Vec<serde_json::Value>> {
        None
    }

    /// GET /annotate
    #[cfg(feature = "browser-core")]
    pub fn annotate_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .annotate_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn annotate_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// GET /feeds
    #[cfg(feature = "browser-core")]
    pub fn feed_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .feed_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn feed_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    // ── TOR-v2 pack ──────────────────────────────────────────────

    /// POST /redirect — rewrite a URL through (defredirect) rules.
    #[cfg(feature = "browser-core")]
    pub fn redirect_apply(&self, req: RedirectRequest) -> UrlRewriteResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let rewritten = inner.pipeline.redirect_apply(&req.url);
        let output = rewritten.clone().unwrap_or_else(|| req.url.clone());
        UrlRewriteResponse {
            input: req.url.clone(),
            output: output.clone(),
            changed: rewritten.is_some() && req.url != output,
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn redirect_apply(&self, req: RedirectRequest) -> UrlRewriteResponse {
        UrlRewriteResponse {
            input: req.url.clone(),
            output: req.url,
            changed: false,
        }
    }

    /// GET /redirect — list rules.
    #[cfg(feature = "browser-core")]
    pub fn redirect_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .redirect_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn redirect_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// POST /url-clean
    #[cfg(feature = "browser-core")]
    pub fn url_clean_apply(&self, req: UrlCleanRequest) -> UrlRewriteResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let cleaned = inner.pipeline.url_clean_apply(&req.url);
        UrlRewriteResponse {
            input: req.url.clone(),
            output: cleaned.clone(),
            changed: cleaned != req.url,
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn url_clean_apply(&self, req: UrlCleanRequest) -> UrlRewriteResponse {
        UrlRewriteResponse {
            input: req.url.clone(),
            output: req.url,
            changed: false,
        }
    }

    #[cfg(feature = "browser-core")]
    pub fn url_clean_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .url_clean_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn url_clean_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// GET /script-policy
    #[cfg(feature = "browser-core")]
    pub fn script_policy_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .script_policy_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(feature = "browser-core")]
    pub fn script_policy_for(&self, host: &str) -> Option<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .script_policy_for(host)
            .and_then(|s| serde_json::to_value(&s).ok())
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn script_policy_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn script_policy_for(&self, _h: &str) -> Option<serde_json::Value> {
        None
    }

    /// GET /bridges
    #[cfg(feature = "browser-core")]
    pub fn bridge_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .bridge_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    /// GET /bridges/torrc — emit torrc block for every enabled bridge.
    #[cfg(feature = "browser-core")]
    pub fn bridges_torrc_block(&self) -> String {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.bridges_torrc_block()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn bridge_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn bridges_torrc_block(&self) -> String {
        String::new()
    }

    // ── Privacy pack ─────────────────────────────────────────────

    /// GET /spoofs
    #[cfg(feature = "browser-core")]
    pub fn spoofs_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .spoofs_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn spoofs_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// GET /spoof?host=…
    #[cfg(feature = "browser-core")]
    pub fn spoof_for(&self, host: &str) -> Option<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .spoof_for(host)
            .and_then(|s| serde_json::to_value(&s).ok())
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn spoof_for(&self, _h: &str) -> Option<serde_json::Value> {
        None
    }

    /// GET /dns
    #[cfg(feature = "browser-core")]
    pub fn dns_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .dns_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn dns_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// GET /dns/:name
    #[cfg(feature = "browser-core")]
    pub fn dns_get(&self, name: &str) -> Option<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .dns_get(name)
            .and_then(|s| serde_json::to_value(&s).ok())
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn dns_get(&self, _n: &str) -> Option<serde_json::Value> {
        None
    }

    /// GET /routing
    #[cfg(feature = "browser-core")]
    pub fn routing_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .routing_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn routing_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// GET /routing/resolve?host=…
    #[cfg(feature = "browser-core")]
    pub fn routing_resolve(&self, host: &str) -> RoutingResolveResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let (rule, via) = inner.pipeline.routing_for(host);
        let (kind, target) = match via {
            nami_core::routing::RouteVia::Direct => ("direct", None),
            nami_core::routing::RouteVia::Tunnel(n) => ("tunnel", Some(n)),
            nami_core::routing::RouteVia::Tor(n) => ("tor", Some(n)),
            nami_core::routing::RouteVia::Socks5(u) => ("socks5", Some(u)),
            nami_core::routing::RouteVia::PluggableTransport(n) => ("pt", Some(n)),
            nami_core::routing::RouteVia::Unknown(s) => ("unknown", Some(s)),
        };
        RoutingResolveResponse {
            host: host.to_owned(),
            rule,
            via_kind: kind.to_owned(),
            via_target: target,
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn routing_resolve(&self, host: &str) -> RoutingResolveResponse {
        RoutingResolveResponse {
            host: host.to_owned(),
            rule: None,
            via_kind: "direct".to_owned(),
            via_target: None,
        }
    }

    /// GET /spaces — list all declared spaces.
    #[cfg(feature = "browser-core")]
    pub fn spaces_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .spaces_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn spaces_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// GET /spaces/:name
    #[cfg(feature = "browser-core")]
    pub fn space_get(&self, name: &str) -> Option<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .space_get(name)
            .and_then(|s| serde_json::to_value(&s).ok())
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn space_get(&self, _n: &str) -> Option<serde_json::Value> {
        None
    }

    /// POST /spaces/:name/activate
    #[cfg(feature = "browser-core")]
    pub fn space_activate(&self, name: &str) -> Option<SpaceActivateResponse> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        if inner.pipeline.space_activate(name) {
            Some(SpaceActivateResponse {
                active: name.to_owned(),
            })
        } else {
            None
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn space_activate(&self, _n: &str) -> Option<SpaceActivateResponse> {
        None
    }

    /// GET /spaces/active
    #[cfg(feature = "browser-core")]
    pub fn space_active(&self) -> SpaceActiveResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        SpaceActiveResponse {
            active: inner.pipeline.space_active(),
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn space_active(&self) -> SpaceActiveResponse {
        SpaceActiveResponse { active: None }
    }

    /// DELETE /spaces/active
    #[cfg(feature = "browser-core")]
    pub fn space_deactivate(&self) {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.space_deactivate();
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn space_deactivate(&self) {}

    /// GET /sidebars[?host=…]
    #[cfg(feature = "browser-core")]
    pub fn sidebars_list(&self, host: Option<&str>) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let specs = match host {
            Some(h) => inner.pipeline.sidebars_visible(h),
            None => inner.pipeline.sidebars_list(),
        };
        specs
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn sidebars_list(&self, _h: Option<&str>) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// GET /splits
    #[cfg(feature = "browser-core")]
    pub fn splits_list(&self) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .splits_list()
            .into_iter()
            .filter_map(|s| serde_json::to_value(&s).ok())
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn splits_list(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// GET /splits/:name
    #[cfg(feature = "browser-core")]
    pub fn split_get(&self, name: &str) -> Option<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .split_get(name)
            .and_then(|s| serde_json::to_value(&s).ok())
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn split_get(&self, _n: &str) -> Option<serde_json::Value> {
        None
    }

    /// POST /js/eval — run script through the active JsRuntime.
    #[cfg(feature = "browser-core")]
    pub fn js_eval(&self, req: JsEvalRequest) -> JsEvalResponse {
        use nami_core::js_runtime::Value as JsValue;
        let inner = self.inner.lock().expect("service mutex poisoned");
        let engine = inner.pipeline.js_engine_name().to_owned();

        // Pack caller-provided JSON vars into the runtime's Value type.
        // We accept only top-level primitives + strings + numbers +
        // bools — MicroEval doesn't walk objects.
        let vars = match &req.vars {
            serde_json::Value::Object(map) => map
                .iter()
                .filter_map(|(k, v)| {
                    let converted = match v {
                        serde_json::Value::Null => Some(JsValue::Null),
                        serde_json::Value::Bool(b) => Some(JsValue::Bool(*b)),
                        serde_json::Value::Number(n) => n.as_f64().map(JsValue::Number),
                        serde_json::Value::String(s) => Some(JsValue::String(s.clone())),
                        _ => None,
                    };
                    converted.map(|v| (k.clone(), v))
                })
                .collect(),
            _ => std::collections::HashMap::new(),
        };

        match inner.pipeline.js_eval(&req.source, req.profile.as_deref(), vars, req.origin) {
            Ok(r) => JsEvalResponse {
                outcome: "ok".into(),
                value: Some(serde_json::to_value(&r.value).unwrap_or_default()),
                fuel_used: r.fuel_used,
                memory_peak: r.memory_peak,
                logs: r.logs,
                engine,
                error: None,
                error_kind: None,
            },
            Err(e) => {
                let kind = match &e {
                    nami_core::js_runtime::EvalError::Parse(_) => "parse",
                    nami_core::js_runtime::EvalError::OutOfFuel { .. } => "out-of-fuel",
                    nami_core::js_runtime::EvalError::OutOfMemory { .. } => "out-of-memory",
                    nami_core::js_runtime::EvalError::PermissionDenied(_) => "permission-denied",
                    nami_core::js_runtime::EvalError::Runtime(_) => "runtime",
                    nami_core::js_runtime::EvalError::Unsupported(_) => "unsupported",
                };
                JsEvalResponse {
                    outcome: "error".into(),
                    value: None,
                    fuel_used: 0,
                    memory_peak: 0,
                    logs: vec![],
                    engine,
                    error: Some(e.to_string()),
                    error_kind: Some(kind.to_owned()),
                }
            }
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn js_eval(&self, _req: JsEvalRequest) -> JsEvalResponse {
        JsEvalResponse {
            outcome: "error".into(),
            value: None,
            fuel_used: 0,
            memory_peak: 0,
            logs: vec![],
            engine: "disabled".into(),
            error: Some("browser-core feature disabled".into()),
            error_kind: Some("unsupported".into()),
        }
    }

    /// POST /find — run find against the last-navigated page.
    #[cfg(feature = "browser-core")]
    pub fn find(&self, req: FindRequest) -> Option<FindResponse> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let sexp = inner.last_outcome.as_ref()?.dom_sexp.clone();
        let profile_name = req
            .profile
            .clone()
            .unwrap_or_else(|| "default".to_owned());
        let spec = inner.pipeline.find_profile(req.profile.as_deref());
        drop(inner);
        let doc = nami_core::lisp::sexp_to_dom(&sexp).ok()?;
        let hits = nami_core::find::find_in_document(&doc, &req.query, &spec);
        Some(FindResponse {
            query: req.query,
            profile: profile_name,
            matches: hits
                .into_iter()
                .map(|m| FindMatchInfo {
                    enclosing_tag: m.enclosing_tag,
                    text_node_index: m.text_node_index,
                    offset: m.offset,
                    matched: m.matched,
                })
                .collect(),
        })
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn find(&self, _req: FindRequest) -> Option<FindResponse> {
        None
    }

    /// GET /zoom?host=…
    #[cfg(feature = "browser-core")]
    pub fn zoom_for(&self, host: &str) -> ZoomResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let (level, text_only) = inner.pipeline.zoom_for(host);
        ZoomResponse {
            host: host.to_owned(),
            level,
            text_only,
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn zoom_for(&self, host: &str) -> ZoomResponse {
        ZoomResponse {
            host: host.to_owned(),
            level: 1.0,
            text_only: false,
        }
    }

    /// GET /snapshot/recipe?host=&name=
    #[cfg(feature = "browser-core")]
    pub fn snapshot_recipe(&self, name: Option<&str>, host: &str) -> Option<SnapshotRecipeResponse> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let spec = inner.pipeline.snapshot_recipe(name, host)?;
        let scale = spec.clamped_scale();
        let quality = spec.clamped_quality();
        Some(SnapshotRecipeResponse {
            name: spec.name,
            region: format!("{:?}", spec.region).to_lowercase(),
            format: format!("{:?}", spec.format).to_lowercase(),
            scale,
            quality,
            selector: spec.selector,
            attest: spec.attest,
        })
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn snapshot_recipe(
        &self,
        _n: Option<&str>,
        _h: &str,
    ) -> Option<SnapshotRecipeResponse> {
        None
    }

    /// GET /pip?host=…
    #[cfg(feature = "browser-core")]
    pub fn pip_for(&self, host: &str) -> PipResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let Some(spec) = inner.pipeline.pip_for(host) else {
            return PipResponse {
                host: host.to_owned(),
                ..PipResponse::default()
            };
        };
        PipResponse {
            host: host.to_owned(),
            name: Some(spec.name),
            selectors: spec.selectors,
            position: format!("{:?}", spec.position).to_lowercase(),
            auto_activate: spec.auto_activate,
            always_on_top: spec.always_on_top,
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn pip_for(&self, host: &str) -> PipResponse {
        PipResponse {
            host: host.to_owned(),
            ..PipResponse::default()
        }
    }

    /// POST /gesture/dispatch — resolve a stroke to a command.
    #[cfg(feature = "browser-core")]
    pub fn gesture_dispatch(&self, req: GestureDispatchRequest) -> GestureDispatchResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        match inner.pipeline.gesture_dispatch(&req.stroke) {
            Some(spec) => GestureDispatchResponse {
                outcome: "run".into(),
                command: Some(spec.command),
            },
            None => GestureDispatchResponse {
                outcome: "miss".into(),
                command: None,
            },
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn gesture_dispatch(&self, _req: GestureDispatchRequest) -> GestureDispatchResponse {
        GestureDispatchResponse {
            outcome: "miss".into(),
            command: None,
        }
    }

    /// GET /boosts
    #[cfg(feature = "browser-core")]
    pub fn boosts_list(&self, host: Option<&str>) -> Vec<BoostInfo> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let specs: Vec<_> = match host {
            Some(h) => inner.pipeline.boosts_applicable(h),
            None => inner.pipeline.boosts_applicable("*all-hosts*"), // filter below
        };
        // If host is None, list ALL boosts regardless of host match.
        let raw = if host.is_some() {
            specs
        } else {
            // Use a wildcard-accepting iteration: applicable("") matches
            // rules with host="" or "*" only; we want every boost.
            inner
                .pipeline
                .boosts_applicable("")
                .into_iter()
                .chain(Vec::new())
                .collect()
        };
        raw.into_iter()
            .map(|s| BoostInfo {
                name: s.name,
                host: s.host,
                enabled: s.enabled,
                has_css: s.css.as_deref().is_some_and(|c| !c.is_empty()),
                has_lisp: s.lisp.as_deref().is_some_and(|c| !c.is_empty()),
                has_js: s.js.as_deref().is_some_and(|c| !c.is_empty()),
                blocker_count: s.blockers.len(),
            })
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn boosts_list(&self, _h: Option<&str>) -> Vec<BoostInfo> {
        Vec::new()
    }

    /// GET /boosts/css?host=…
    #[cfg(feature = "browser-core")]
    pub fn boost_css(&self, host: &str) -> String {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.boost_css(host)
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn boost_css(&self, _h: &str) -> String {
        String::new()
    }

    /// POST /boosts/:name/enabled
    #[cfg(feature = "browser-core")]
    pub fn boost_set_enabled(&self, name: &str, req: BoostToggleRequest) -> bool {
        let mut inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.boost_set_enabled(name, req.enabled)
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn boost_set_enabled(&self, _n: &str, _r: BoostToggleRequest) -> bool {
        false
    }

    /// GET /session/tabs
    #[cfg(feature = "browser-core")]
    pub fn session_open(&self) -> Vec<SessionTabInfo> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .session_snapshot()
            .into_iter()
            .map(|t| SessionTabInfo {
                url: t.url.to_string(),
                title: t.title,
                closed_at: t.closed_at,
                pinned: t.pinned,
            })
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn session_open(&self) -> Vec<SessionTabInfo> {
        Vec::new()
    }

    /// GET /session/closed
    #[cfg(feature = "browser-core")]
    pub fn session_closed(&self) -> Vec<SessionTabInfo> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .pipeline
            .session_closed_tabs()
            .into_iter()
            .map(|t| SessionTabInfo {
                url: t.url.to_string(),
                title: t.title,
                closed_at: t.closed_at,
                pinned: t.pinned,
            })
            .collect()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn session_closed(&self) -> Vec<SessionTabInfo> {
        Vec::new()
    }

    /// POST /session/undo-close
    #[cfg(feature = "browser-core")]
    pub fn session_undo_close(&self) -> Option<SessionTabInfo> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.session_undo_close().map(|t| SessionTabInfo {
            url: t.url.to_string(),
            title: t.title,
            closed_at: t.closed_at,
            pinned: t.pinned,
        })
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn session_undo_close(&self) -> Option<SessionTabInfo> {
        None
    }

    /// GET /storage/:name/index/:path/range?lo=&hi=
    #[cfg(feature = "browser-core")]
    pub fn storage_by_index_range(
        &self,
        store: &str,
        path: &str,
        lo: &str,
        hi: &str,
    ) -> Option<Vec<StorageEntry>> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let hits = inner.pipeline.storage_by_index_range(store, path, lo, hi)?;
        Some(
            hits.into_iter()
                .map(|(key, value)| StorageEntry { key, value })
                .collect(),
        )
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn storage_by_index_range(
        &self,
        _s: &str,
        _p: &str,
        _lo: &str,
        _hi: &str,
    ) -> Option<Vec<StorageEntry>> {
        None
    }

    /// GET /i18n/:namespace?locale=&key=
    #[cfg(feature = "browser-core")]
    pub fn i18n_get(&self, namespace: &str, locale: &str, key: &str) -> I18nResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let value = inner.pipeline.i18n_get(namespace, locale, key);
        let resolved = value != key;
        I18nResponse {
            namespace: namespace.to_owned(),
            locale: locale.to_owned(),
            value,
            resolved,
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn i18n_get(&self, namespace: &str, locale: &str, key: &str) -> I18nResponse {
        I18nResponse {
            namespace: namespace.to_owned(),
            locale: locale.to_owned(),
            value: key.to_owned(),
            resolved: false,
        }
    }

    /// GET /i18n/:namespace/coverage?locale=
    #[cfg(feature = "browser-core")]
    pub fn i18n_coverage(&self, namespace: &str, locale: &str) -> I18nCoverage {
        let inner = self.inner.lock().expect("service mutex poisoned");
        I18nCoverage {
            namespace: namespace.to_owned(),
            locale: locale.to_owned(),
            available_locales: inner.pipeline.i18n_locales(namespace),
            missing_keys: inner.pipeline.i18n_missing(namespace, locale),
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn i18n_coverage(&self, namespace: &str, locale: &str) -> I18nCoverage {
        I18nCoverage {
            namespace: namespace.to_owned(),
            locale: locale.to_owned(),
            available_locales: vec![],
            missing_keys: vec![],
        }
    }

    /// GET /security-policy?host=…
    #[cfg(feature = "browser-core")]
    pub fn security_policy_for(&self, host: &str) -> SecurityPolicyResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let policy_name = inner.pipeline.security_policy_for(host).map(|s| s.name);
        let headers = inner.pipeline.security_policy_headers(host).headers;
        SecurityPolicyResponse {
            host: host.to_owned(),
            policy_name,
            headers,
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn security_policy_for(&self, host: &str) -> SecurityPolicyResponse {
        SecurityPolicyResponse {
            host: host.to_owned(),
            policy_name: None,
            headers: vec![],
        }
    }

    /// GET /storage/:name/index — list every declared index and its
    /// distinct projected values. Useful for range scans + inspector
    /// surfaces.
    #[cfg(feature = "browser-core")]
    pub fn storage_index_summary(&self, store: &str) -> Option<Vec<crate::api::StorageIndexSummary>> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let s = inner.pipeline.get_store(store)?;
        let paths = s.index_paths();
        Some(
            paths
                .into_iter()
                .map(|p| crate::api::StorageIndexSummary {
                    distinct_values: s.index_values(&p).unwrap_or_default(),
                    path: p,
                })
                .collect(),
        )
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn storage_index_summary(&self, _s: &str) -> Option<Vec<crate::api::StorageIndexSummary>> {
        None
    }

    /// GET /storage/:name/index/:path?value=V — every entry whose
    /// projected value at `path` equals V. Returns None when the
    /// store or path isn't declared.
    #[cfg(feature = "browser-core")]
    pub fn storage_by_index(
        &self,
        store: &str,
        path: &str,
        value: &str,
    ) -> Option<Vec<StorageEntry>> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let s = inner.pipeline.get_store(store)?;
        let hits = s.by_index(path, value)?;
        Some(
            hits.into_iter()
                .map(|(key, value)| StorageEntry { key, value })
                .collect(),
        )
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn storage_by_index(
        &self,
        _s: &str,
        _p: &str,
        _v: &str,
    ) -> Option<Vec<StorageEntry>> {
        None
    }

    /// GET /commands — full command+binding inventory.
    #[cfg(feature = "browser-core")]
    pub fn commands_list(&self) -> Vec<CommandInfo> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner.pipeline.commands_inventory()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn commands_list(&self) -> Vec<CommandInfo> {
        Vec::new()
    }

    /// POST /commands/dispatch — simulate a key sequence.
    #[cfg(feature = "browser-core")]
    pub fn dispatch_key(&self, req: DispatchKeyRequest) -> DispatchKeyResponse {
        let mode = req.mode.as_deref().unwrap_or("any");
        let inner = self.inner.lock().expect("service mutex poisoned");
        match inner.pipeline.dispatch_key(&req.typed, mode) {
            crate::webview::substrate::KeyDispatch::Run { bind, command } => {
                DispatchKeyResponse {
                    outcome: "run".into(),
                    command: Some(bind.command.clone()),
                    action: command.as_ref().and_then(|c| c.action.clone()),
                    body: command.as_ref().and_then(|c| c.body.clone()),
                    key: Some(bind.canonical_key()),
                }
            }
            crate::webview::substrate::KeyDispatch::Prefix => DispatchKeyResponse {
                outcome: "prefix".into(),
                command: None,
                action: None,
                body: None,
                key: None,
            },
            crate::webview::substrate::KeyDispatch::Miss => DispatchKeyResponse {
                outcome: "miss".into(),
                command: None,
                action: None,
                body: None,
                key: None,
            },
        }
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn dispatch_key(&self, _req: DispatchKeyRequest) -> DispatchKeyResponse {
        DispatchKeyResponse {
            outcome: "miss".into(),
            command: None,
            action: None,
            body: None,
            key: None,
        }
    }

    /// GET /bookmarks — list all (all folders).
    pub fn bookmarks_list(&self) -> Vec<BookmarkInfo> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        inner
            .bookmarks
            .list(None)
            .iter()
            .map(|b| BookmarkInfo::from_bookmark(*b))
            .collect()
    }

    /// POST /bookmarks — add. Returns true if newly added, false if
    /// the URL was already bookmarked.
    pub fn bookmark_add(&self, req: AddBookmarkRequest) -> Result<bool> {
        let url = Url::parse(&req.url)
            .or_else(|_| Url::parse(&format!("https://{}", req.url)))?;
        let title = req
            .title
            .unwrap_or_else(|| url.to_string());
        let mut bm = Bookmark::new(title, url);
        if let Some(folder) = req.folder {
            bm = bm.with_folder(folder);
        }
        if !req.tags.is_empty() {
            bm = bm.with_tags(req.tags);
        }
        let mut inner = self.inner.lock().expect("service mutex poisoned");
        Ok(inner.bookmarks.add(bm))
    }

    /// DELETE /bookmarks/:url — remove. Returns true if removed.
    pub fn bookmark_remove(&self, url_str: &str) -> Result<bool> {
        let url = Url::parse(url_str)
            .or_else(|_| Url::parse(&format!("https://{}", url_str)))?;
        let mut inner = self.inner.lock().expect("service mutex poisoned");
        Ok(inner.bookmarks.remove(&url))
    }

    /// GET /report — the structured substrate report from the last
    /// navigate. Returns 404-shaped `None` when no navigate has happened.
    pub fn last_report(&self) -> Option<ReportResponse> {
        #[cfg(feature = "browser-core")]
        {
            let inner = self.inner.lock().expect("service mutex poisoned");
            inner.last_outcome.as_ref().map(ReportResponse::from_outcome)
        }

        #[cfg(not(feature = "browser-core"))]
        None
    }

    /// GET /dom — last navigated page as S-expression (Lisp space).
    pub fn last_dom_sexp(&self) -> Option<String> {
        #[cfg(feature = "browser-core")]
        {
            let inner = self.inner.lock().expect("service mutex poisoned");
            return inner.last_outcome.as_ref().map(|o| o.dom_sexp.clone());
        }

        #[cfg(not(feature = "browser-core"))]
        None
    }

    /// GET /accessibility — AX tree of the last navigated DOM.
    /// Canonical n-* vocab IS the ARIA role map, so every
    /// normalize-matched page yields a valid AccessKit-shaped tree.
    ///
    /// Reconstitutes the Document from the cached dom_sexp rather
    /// than keeping a second copy around — the overhead is trivial
    /// (O(DOM) string parse) and it keeps NavigateOutcome lean.
    #[cfg(feature = "browser-core")]
    pub fn last_accessibility_tree(&self) -> Option<serde_json::Value> {
        let inner = self.inner.lock().expect("service mutex poisoned");
        let sexp = inner.last_outcome.as_ref()?.dom_sexp.clone();
        drop(inner);
        let doc = nami_core::lisp::sexp_to_dom(&sexp).ok()?;
        let tree = nami_core::accessibility::ax_tree(&doc);
        serde_json::to_value(&tree).ok()
    }

    #[cfg(not(feature = "browser-core"))]
    pub fn last_accessibility_tree(&self) -> Option<serde_json::Value> {
        None
    }

    /// GET /rules — inventory of every loaded DSL form by name.
    pub fn rules_inventory(&self) -> RulesInventory {
        #[cfg(feature = "browser-core")]
        {
            let inner = self.inner.lock().expect("service mutex poisoned");
            return inner.pipeline.rules_inventory();
        }
        #[cfg(not(feature = "browser-core"))]
        RulesInventory::default()
    }

    /// GET /state — current state store snapshot (across all navigates).
    pub fn state_snapshot(&self) -> Vec<StateCellValue> {
        #[cfg(feature = "browser-core")]
        {
            let inner = self.inner.lock().expect("service mutex poisoned");
            inner
                .pipeline
                .state_snapshot()
                .into_iter()
                .map(|(name, value)| StateCellValue { name, value })
                .collect()
        }

        #[cfg(not(feature = "browser-core"))]
        Vec::new()
    }

    #[allow(dead_code)]
    fn last_url(&self, inner: &Inner) -> Option<String> {
        #[cfg(feature = "browser-core")]
        {
            return inner.last_outcome.as_ref().map(|o| o.final_url.to_string());
        }

        #[cfg(not(feature = "browser-core"))]
        {
            let _ = inner;
            None
        }
    }
}

impl Default for NamimadoService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "browser-core")]
fn pack_llm_response(r: nami_core::llm::LlmResponse, engine: String) -> LlmResponseDto {
    LlmResponseDto {
        outcome: "ok".into(),
        content: Some(r.content),
        input_tokens: r.input_tokens,
        output_tokens: r.output_tokens,
        model: Some(r.model),
        stopped: Some(match r.stopped {
            nami_core::llm::StopReason::EndTurn => "end-turn",
            nami_core::llm::StopReason::StopSequence => "stop-sequence",
            nami_core::llm::StopReason::MaxTokens => "max-tokens",
            nami_core::llm::StopReason::Other => "other",
        }.to_owned()),
        engine,
        error: None,
    }
}

#[cfg(feature = "browser-core")]
fn pack_llm_error(e: nami_core::llm::LlmError, engine: String) -> LlmResponseDto {
    LlmResponseDto {
        outcome: "error".into(),
        content: None,
        input_tokens: 0,
        output_tokens: 0,
        model: None,
        stopped: None,
        engine,
        error: Some(e.to_string()),
    }
}

#[cfg(not(feature = "browser-core"))]
fn disabled_response() -> LlmResponseDto {
    LlmResponseDto {
        outcome: "error".into(),
        content: None,
        input_tokens: 0,
        output_tokens: 0,
        model: None,
        stopped: None,
        engine: "disabled".into(),
        error: Some("browser-core feature disabled".into()),
    }
}

fn compile_features() -> Vec<String> {
    let mut out = Vec::new();
    if cfg!(feature = "browser-core") {
        out.push("browser-core".into());
    }
    if cfg!(feature = "gpu-chrome") {
        out.push("gpu-chrome".into());
    }
    if cfg!(feature = "http-server") {
        out.push("http-server".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_status_reports_features_and_version() {
        let svc = NamimadoService::new();
        let s = svc.status();
        assert_eq!(s.service, "namimado");
        assert_eq!(s.version, env!("CARGO_PKG_VERSION"));
        assert!(s.last_url.is_none());
    }

    #[test]
    fn service_report_is_none_before_navigate() {
        let svc = NamimadoService::new();
        assert!(svc.last_report().is_none());
    }

    #[test]
    fn service_clones_share_state() {
        // The same Arc<Mutex<Inner>> is visible via every clone.
        let a = NamimadoService::new();
        let b = a.clone();
        assert_eq!(a.status().version, b.status().version);
    }

    #[test]
    fn reload_increments_count_and_returns_fresh_inventory() {
        let svc = NamimadoService::new();
        let before = svc.status();
        assert_eq!(before.reload_count, 0);

        let r = svc.reload();
        assert_eq!(r.reload_count, 1);
        // Every feature-enabled build reloads; the no-browser-core
        // build returns reloaded:false (see ReloadResponse).
        assert_eq!(r.reloaded, cfg!(feature = "browser-core"));

        let after = svc.status();
        assert_eq!(after.reload_count, 1);
        assert!(after.loaded_at_epoch >= before.loaded_at_epoch);
    }

    #[test]
    fn reload_clears_last_outcome() {
        // No navigate has happened yet → report is None.
        let svc = NamimadoService::new();
        assert!(svc.last_report().is_none());

        // After a reload, the slot is still None (nothing to clear,
        // but the API shape stays consistent).
        svc.reload();
        assert!(svc.last_report().is_none());
    }

    #[test]
    fn repeated_reloads_are_sequenceable() {
        let svc = NamimadoService::new();
        for expected in 1..=3 {
            let r = svc.reload();
            assert_eq!(r.reload_count, expected);
        }
        assert_eq!(svc.status().reload_count, 3);
    }

    #[test]
    fn history_starts_empty_and_grows_on_navigate_like_calls() {
        // navigate() itself needs a server; manually exercise the
        // auto-record path by calling into the history mutator the
        // same way navigate() does — via the Inner lock.
        let svc = NamimadoService::new();
        assert!(svc.history_recent(50).is_empty());
        // Simulate what navigate() does on success:
        {
            let mut inner = svc.inner.lock().unwrap();
            let url = Url::parse("https://example.com/").unwrap();
            inner.history.record_visit("Example", url);
        }
        let recent = svc.history_recent(50);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].title, "Example");
        assert!(recent[0].url.starts_with("https://example.com"));
    }

    #[test]
    fn history_search_matches_title_or_url() {
        let svc = NamimadoService::new();
        {
            let mut inner = svc.inner.lock().unwrap();
            inner.history.record_visit("Rust Forum", Url::parse("https://users.rust-lang.org/").unwrap());
            inner.history.record_visit("News", Url::parse("https://news.ycombinator.com/").unwrap());
        }
        let rust = svc.history_search("rust");
        assert_eq!(rust.len(), 1);
        let ycom = svc.history_search("ycombinator");
        assert_eq!(ycom.len(), 1);
        let miss = svc.history_search("nothing-to-find");
        assert!(miss.is_empty());
    }

    #[test]
    fn history_clear_wipes_everything() {
        let svc = NamimadoService::new();
        {
            let mut inner = svc.inner.lock().unwrap();
            inner.history.record_visit("t", Url::parse("https://a/").unwrap());
            inner.history.record_visit("u", Url::parse("https://b/").unwrap());
        }
        assert_eq!(svc.history_recent(10).len(), 2);
        svc.history_clear();
        assert!(svc.history_recent(10).is_empty());
    }

    #[test]
    fn bookmark_add_roundtrips_and_prevents_duplicates() {
        let svc = NamimadoService::new();
        let req = AddBookmarkRequest {
            url: "https://example.com/".into(),
            title: Some("Example".into()),
            folder: None,
            tags: vec!["test".into()],
        };
        let first = svc.bookmark_add(req.clone()).unwrap();
        assert!(first, "first add should return true (newly added)");
        let second = svc.bookmark_add(req).unwrap();
        assert!(!second, "second add should return false (already present)");
        let list = svc.bookmarks_list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Example");
        assert_eq!(list[0].tags, vec!["test".to_owned()]);
    }

    #[test]
    fn bookmark_remove_finds_by_url() {
        let svc = NamimadoService::new();
        svc.bookmark_add(AddBookmarkRequest {
            url: "https://example.com/".into(),
            title: Some("Example".into()),
            folder: None,
            tags: vec![],
        })
        .unwrap();
        assert_eq!(svc.bookmarks_list().len(), 1);
        let removed = svc.bookmark_remove("https://example.com/").unwrap();
        assert!(removed);
        assert!(svc.bookmarks_list().is_empty());
    }

    #[test]
    fn bookmark_add_without_scheme_defaults_to_https() {
        let svc = NamimadoService::new();
        let added = svc
            .bookmark_add(AddBookmarkRequest {
                url: "example.com".into(),
                title: Some("t".into()),
                folder: None,
                tags: vec![],
            })
            .unwrap();
        assert!(added);
        let list = svc.bookmarks_list();
        assert!(list[0].url.starts_with("https://example.com"));
    }

    #[test]
    fn storage_list_returns_vec_without_panic() {
        // Smoke — even with no (defstorage …) declared, the surface
        // must not panic. It returns an empty Vec.
        let svc = NamimadoService::new();
        let stores = svc.storage_list();
        assert!(stores.is_empty() || stores.iter().all(|s| !s.name.is_empty()));
    }

    #[test]
    fn storage_get_on_unknown_store_is_none() {
        let svc = NamimadoService::new();
        assert!(svc.storage_get("nonexistent", "k").is_none());
        assert!(svc.storage_entries("nonexistent").is_none());
        assert!(!svc.storage_delete("nonexistent", "k"));
    }

    #[test]
    fn omnibox_empty_query_returns_zero_suggestions() {
        let svc = NamimadoService::new();
        let resp = svc.omnibox("", None);
        assert!(resp.suggestions.is_empty());
        assert_eq!(resp.query, "");
    }

    #[test]
    fn omnibox_direct_url_emits_navigate() {
        let svc = NamimadoService::new();
        let resp = svc.omnibox("example.com", None);
        // default profile always emits search providers, plus a
        // navigate suggestion for the URL-shaped query.
        assert!(resp
            .suggestions
            .iter()
            .any(|s| s.kind == "navigate" && s.action == "navigate:https://example.com"));
    }

    #[test]
    fn verify_signed_extension_roundtrips_through_service() {
        use nami_core::extension::{signing_key_from_seed, sign, ExtensionSpec, SignedExtension};
        let svc = NamimadoService::new();

        let key = signing_key_from_seed(&[13u8; 32]);
        let spec = ExtensionSpec {
            name: "smoke-ext".into(),
            version: "1.0.0".into(),
            description: None,
            author: None,
            homepage_url: None,
            icon: None,
            permissions: vec![],
            host_permissions: vec![],
            rules: vec![],
            enabled: true,
        };
        let bundle = sign(&spec, &key);
        let pubkey = bundle.public_key.clone();
        let signed = SignedExtension { spec, signature: bundle };

        // Untrusted by default.
        let r1 = svc.verify_signed_extension(&signed);
        assert_eq!(r1.status, "valid-but-untrusted");
        assert_eq!(r1.public_key.as_deref(), Some(pubkey.as_str()));

        // Trust + retry.
        assert!(svc.trust_pubkey(TrustdbKeyRequest {
            public_key: pubkey.clone(),
            signed_by: None,
        }));
        let r2 = svc.verify_signed_extension(&signed);
        assert_eq!(r2.status, "trusted");

        // Revoke + retry.
        assert!(svc.revoke_pubkey(&pubkey));
        let r3 = svc.verify_signed_extension(&signed);
        assert_eq!(r3.status, "valid-but-untrusted");
    }

    #[test]
    fn i18n_falls_back_to_raw_key_when_empty() {
        let svc = NamimadoService::new();
        let r = svc.i18n_get("core", "en", "nothing.here");
        assert_eq!(r.value, "nothing.here");
        assert!(!r.resolved);
    }

    #[test]
    fn i18n_coverage_empty_when_no_bundles() {
        let svc = NamimadoService::new();
        let c = svc.i18n_coverage("core", "ja");
        assert!(c.available_locales.is_empty());
        assert!(c.missing_keys.is_empty());
    }

    #[test]
    fn security_policy_returns_empty_when_no_rule_matches() {
        let svc = NamimadoService::new();
        let r = svc.security_policy_for("example.com");
        assert!(r.headers.is_empty());
        assert!(r.policy_name.is_none());
    }

    #[test]
    fn find_returns_none_before_navigate() {
        let svc = NamimadoService::new();
        assert!(svc.find(FindRequest {
            query: "hello".into(),
            profile: None,
        }).is_none());
    }

    #[test]
    fn zoom_defaults_to_one_when_no_rules() {
        let svc = NamimadoService::new();
        let r = svc.zoom_for("example.com");
        assert!((r.level - 1.0).abs() < f32::EPSILON);
        assert!(!r.text_only);
    }

    #[test]
    fn pip_default_has_video_selector() {
        let svc = NamimadoService::new();
        let r = svc.pip_for("example.com");
        // Default profile auto-registers so we always get the `video` selector.
        assert!(r.selectors.iter().any(|s| s == "video"));
    }

    #[test]
    fn gesture_miss_when_registry_empty() {
        let svc = NamimadoService::new();
        let r = svc.gesture_dispatch(GestureDispatchRequest {
            stroke: "U L".into(),
        });
        assert_eq!(r.outcome, "miss");
    }

    #[test]
    fn snapshot_recipe_returns_default_when_host_matches() {
        let svc = NamimadoService::new();
        // Default snapshot profile auto-registers; matches "*".
        let r = svc.snapshot_recipe(None, "anywhere.com").unwrap();
        assert_eq!(r.name, "default");
    }

    #[test]
    fn session_roundtrip_record_undo() {
        use nami_core::session::TabRecord;
        use url::Url;
        let svc = NamimadoService::new();
        let tab = TabRecord {
            url: Url::parse("https://example.com/").unwrap(),
            title: "Example".into(),
            closed_at: 1,
            pinned: false,
        };
        // Get the pipeline via reload-induced reinitialization isn't
        // great; use the direct substrate handle: record via pipeline.
        {
            let inner = svc.inner.lock().unwrap();
            inner.pipeline.session_record_close(tab.clone());
        }
        assert_eq!(svc.session_closed().len(), 1);
        let undone = svc.session_undo_close().unwrap();
        assert_eq!(undone.url, "https://example.com/");
        assert!(svc.session_closed().is_empty());
    }

    #[test]
    fn js_eval_microeval_arithmetic_roundtrips() {
        let svc = NamimadoService::new();
        let req = JsEvalRequest {
            source: "1 + 2 * 3".into(),
            profile: None,
            vars: serde_json::Value::Null,
            origin: None,
        };
        let r = svc.js_eval(req);
        assert_eq!(r.outcome, "ok");
        assert_eq!(r.value, Some(serde_json::json!(7.0)));
        assert_eq!(r.engine, "micro-eval");
    }

    #[test]
    fn js_eval_reports_error_on_bad_input() {
        let svc = NamimadoService::new();
        let req = JsEvalRequest {
            source: "let x = 1;".into(),
            profile: None,
            vars: serde_json::Value::Null,
            origin: None,
        };
        let r = svc.js_eval(req);
        assert_eq!(r.outcome, "error");
        assert!(r.error.is_some());
    }

    #[test]
    fn ai_pack_empty_without_declarations() {
        let svc = NamimadoService::new();
        assert!(svc.llm_provider_list().is_empty());
        assert!(svc.summarize_list().is_empty());
        assert!(svc.chat_list().is_empty());
        assert!(svc.llm_completion_list().is_empty());
    }

    #[test]
    fn ai_calls_report_error_when_profile_missing() {
        let svc = NamimadoService::new();
        let r = svc.summarize_run(SummarizeRequest {
            profile: "nope".into(),
            source: "hi".into(),
        });
        assert_eq!(r.outcome, "error");
        assert!(r.error.is_some());
    }

    #[test]
    fn credentials_pack_empty_without_declarations() {
        let svc = NamimadoService::new();
        assert!(svc.autofill_list().is_empty());
        assert!(svc.password_list().is_empty());
        assert!(svc.passwords_for("example.com").is_empty());
        assert!(svc.auth_saver_list().is_empty());
        assert!(svc.auth_saver_for("example.com").is_none());
        assert!(svc.secure_note_list().is_empty());
        assert!(svc.passkey_list().is_empty());
        assert!(svc.passkeys_for("example.com").is_empty());
    }

    #[test]
    fn mobile_download_pack_empty_without_declarations() {
        let svc = NamimadoService::new();
        assert!(svc.share_list().is_empty());
        assert!(svc.offline_list().is_empty());
        assert!(svc.pull_refresh_list().is_empty());
        // Download auto-registers a default profile so list is not empty,
        // but get("nonexistent") returns None.
        assert!(svc.download_get("nonexistent").is_none());
    }

    #[test]
    fn download_default_profile_auto_loads() {
        let svc = NamimadoService::new();
        let list = svc.download_list();
        assert!(!list.is_empty(), "default download profile should auto-register");
    }

    #[test]
    fn pull_refresh_resolve_empty_host_is_none() {
        let svc = NamimadoService::new();
        assert!(svc.pull_refresh_for("anywhere.com").is_none());
    }

    #[test]
    fn reading_and_tor_packs_empty_without_declarations() {
        let svc = NamimadoService::new();
        // Annotate + feed empty.
        assert!(svc.annotate_list().is_empty());
        assert!(svc.feed_list().is_empty());
        // TOR-v2.
        assert!(svc.redirect_list().is_empty());
        assert!(svc.url_clean_list().is_empty());
        assert!(svc.script_policy_list().is_empty());
        assert!(svc.bridge_list().is_empty());
        assert!(svc.bridges_torrc_block().is_empty());
    }

    #[test]
    fn url_clean_apply_passes_through_when_no_rules() {
        let svc = NamimadoService::new();
        let r = svc.url_clean_apply(UrlCleanRequest {
            url: "https://example.com/x?utm_source=twitter".into(),
        });
        assert!(!r.changed);
        assert_eq!(r.output, r.input);
    }

    #[test]
    fn redirect_apply_passes_through_when_no_rules() {
        let svc = NamimadoService::new();
        let r = svc.redirect_apply(RedirectRequest {
            url: "https://youtube.com/watch?v=x".into(),
        });
        assert!(!r.changed);
        assert_eq!(r.output, r.input);
    }

    #[test]
    fn outline_returns_none_before_navigate() {
        let svc = NamimadoService::new();
        assert!(svc
            .outline_extract(OutlineRequest { profile: None })
            .is_none());
    }

    #[test]
    fn privacy_pack_empty_without_declarations() {
        let svc = NamimadoService::new();
        assert!(svc.spoofs_list().is_empty());
        assert!(svc.dns_list().is_empty());
        assert!(svc.routing_list().is_empty());
        assert!(svc.spoof_for("example.com").is_none());
    }

    #[test]
    fn routing_resolve_defaults_to_direct() {
        let svc = NamimadoService::new();
        let r = svc.routing_resolve("example.com");
        assert_eq!(r.via_kind, "direct");
        assert!(r.rule.is_none());
        assert!(r.via_target.is_none());
    }

    #[test]
    fn spaces_list_empty_without_declarations() {
        let svc = NamimadoService::new();
        assert!(svc.spaces_list().is_empty());
    }

    #[test]
    fn space_activate_unknown_returns_none() {
        let svc = NamimadoService::new();
        assert!(svc.space_activate("nonexistent").is_none());
    }

    #[test]
    fn space_active_starts_null() {
        let svc = NamimadoService::new();
        let r = svc.space_active();
        assert!(r.active.is_none());
    }

    #[test]
    fn sidebars_list_empty_when_none_declared() {
        let svc = NamimadoService::new();
        assert!(svc.sidebars_list(None).is_empty());
        assert!(svc.sidebars_list(Some("example.com")).is_empty());
    }

    #[test]
    fn splits_list_empty_when_none_declared() {
        let svc = NamimadoService::new();
        assert!(svc.splits_list().is_empty());
        assert!(svc.split_get("anything").is_none());
    }

    #[test]
    fn js_eval_passes_vars_through_context() {
        let svc = NamimadoService::new();
        let req = JsEvalRequest {
            source: r#""hi " + name"#.into(),
            profile: None,
            vars: serde_json::json!({"name": "world"}),
            origin: None,
        };
        let r = svc.js_eval(req);
        assert_eq!(r.outcome, "ok");
        assert_eq!(r.value, Some(serde_json::json!("hi world")));
    }

    #[test]
    fn boosts_list_is_empty_with_no_specs_declared() {
        let svc = NamimadoService::new();
        let v = svc.boosts_list(None);
        assert!(v.is_empty());
    }

    #[test]
    fn storage_range_on_unknown_store_is_none() {
        let svc = NamimadoService::new();
        assert!(svc
            .storage_by_index_range("nope", "anything", "a", "z")
            .is_none());
    }

    #[test]
    fn verify_rejects_tampered_signed_extension() {
        use nami_core::extension::{signing_key_from_seed, sign, ExtensionSpec, SignedExtension};
        let svc = NamimadoService::new();
        let key = signing_key_from_seed(&[5u8; 32]);
        let spec = ExtensionSpec {
            name: "tamper".into(),
            version: "1.0.0".into(),
            description: None,
            author: None,
            homepage_url: None,
            icon: None,
            permissions: vec![],
            host_permissions: vec![],
            rules: vec!["original".into()],
            enabled: true,
        };
        let bundle = sign(&spec, &key);
        let mut tampered = spec.clone();
        tampered.rules.push("injected".into());
        let signed = SignedExtension { spec: tampered, signature: bundle };
        let r = svc.verify_signed_extension(&signed);
        assert_eq!(r.status, "invalid");
        assert!(r
            .detail
            .as_deref()
            .unwrap_or("")
            .contains("tampered"));
    }

    #[test]
    fn omnibox_picks_up_bookmarks_on_match() {
        let svc = NamimadoService::new();
        svc.bookmark_add(AddBookmarkRequest {
            url: "https://example.com/docs".into(),
            title: Some("Docs Home".into()),
            folder: None,
            tags: vec![],
        })
        .unwrap();
        let resp = svc.omnibox("Docs", None);
        assert!(resp.suggestions.iter().any(|s| s.kind == "bookmark"));
    }

    #[test]
    fn storage_set_on_unknown_store_is_false() {
        let svc = NamimadoService::new();
        let ok = svc.storage_set(
            "nonexistent",
            StorageSetRequest {
                key: "k".into(),
                value: serde_json::json!(1),
            },
        );
        assert!(!ok);
    }
}
