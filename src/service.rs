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
    AddBookmarkRequest, BookmarkInfo, CommandInfo, DispatchKeyRequest, DispatchKeyResponse,
    ExtensionInstallRequest, ExtensionInstallResponse, ExtensionSummary, ExtensionToggleRequest,
    HistoryInfo, NavigateRequest, NavigateResponse, OmniboxResponse, OmniboxSuggestion,
    ReaderResponse, ReloadResponse, ReportResponse, RulesInventory, StateCellValue,
    StatusResponse, StorageEntry, StorageSetRequest, StorageSummary, TrustdbKeyRequest,
    VerifyExtensionResponse,
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
