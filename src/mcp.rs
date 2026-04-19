//! MCP server for Namimado desktop browser via kaname.
//!
//! Exposes browser automation tools over the Model Context Protocol
//! (stdio transport), allowing AI assistants to control tabs, navigate
//! pages, and manage bookmarks.

use kaname::rmcp;
use kaname::ToolResponse;
use rmcp::{
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::config::NamimadoConfig;
use crate::service::NamimadoService;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
struct NewTabRequest {
    /// URL to open in the new tab. Defaults to the homepage.
    url: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CloseTabRequest {
    /// Tab index to close. Defaults to the active tab.
    index: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NavigateRequest {
    /// URL to navigate the active tab to.
    url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AddBookmarkRequest {
    /// URL to bookmark. Defaults to the active tab's URL.
    url: Option<String>,
    /// Bookmark title.
    title: Option<String>,
    /// Tags for the bookmark.
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetBookmarksRequest {
    /// Optional search query — matches title, URL, or tags.
    query: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TrustPubkeyRequest {
    public_key: String,
    #[serde(default)]
    signed_by: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RevokePubkeyRequest {
    public_key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct OmniboxToolRequest {
    /// User query string.
    q: String,
    /// Named (defomnibox) profile. None → first registered.
    profile: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DispatchKeyToolRequest {
    /// Typed-so-far sequence (Vim-style space-separated chords OK).
    typed: String,
    /// Dispatch mode — "normal", "insert", "visual", "any".
    mode: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReaderRequest {
    /// Named (defreader) profile. None uses the host-matching default.
    name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExtensionNameRequest {
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExtensionToggleToolRequest {
    name: String,
    enabled: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExtensionInstallToolRequest {
    /// Raw tatara-lisp source containing at least one (defextension …)
    /// form. Other def* forms in the same source are installed too.
    lisp_source: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StorageStoreRequest {
    /// Store name, as declared by `(defstorage :name …)`.
    store: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StorageKeyRequest {
    store: String,
    key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StorageSetToolRequest {
    store: String,
    key: String,
    /// Arbitrary JSON value. Use `{"_lisp": "(quote …)"}` to persist tatara-lisp.
    value: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct HistorySearchRequest {
    /// Query — matches title or URL substring.
    query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ConfigGetRequest {
    /// Config key (dot-separated path).
    key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ConfigSetRequest {
    /// Config key.
    key: String,
    /// Value to set (as string).
    value: String,
}

// ---------------------------------------------------------------------------
// MCP Service
// ---------------------------------------------------------------------------

/// Namimado browser MCP server.
pub struct NamimadoMcpServer {
    tool_router: ToolRouter<Self>,
    config: NamimadoConfig,
    service: NamimadoService,
}

impl std::fmt::Debug for NamimadoMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NamimadoMcpServer").finish()
    }
}

#[tool_router]
impl NamimadoMcpServer {
    pub fn new(config: NamimadoConfig) -> Self {
        Self {
            tool_router: Self::tool_router(),
            config,
            service: NamimadoService::new(),
        }
    }

    // -- Standard tools --

    #[tool(description = "Namimado status — version, enabled features, last URL fetched.")]
    async fn status(&self) -> Result<CallToolResult, McpError> {
        // Delegates to NamimadoService — byte-identical with GET /status.
        let status = self.service.status();
        Ok(ToolResponse::success(
            &serde_json::to_value(&status).unwrap_or_default(),
        ))
    }

    #[tool(description = "Get the Namimado version.")]
    async fn version(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(&serde_json::json!({
            "name": "namimado",
            "version": env!("CARGO_PKG_VERSION"),
        })))
    }

    #[tool(description = "Get a configuration value by key.")]
    async fn config_get(
        &self,
        Parameters(req): Parameters<ConfigGetRequest>,
    ) -> Result<CallToolResult, McpError> {
        let json = serde_json::to_value(&self.config).unwrap_or_default();
        let value = req
            .key
            .split('.')
            .fold(Some(&json), |v, k| v.and_then(|v| v.get(k)));
        match value {
            Some(v) => Ok(ToolResponse::success(v)),
            None => Ok(ToolResponse::error(&format!("Key '{}' not found", req.key))),
        }
    }

    #[tool(description = "Set a configuration value (runtime only, not persisted).")]
    async fn config_set(
        &self,
        Parameters(req): Parameters<ConfigSetRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::text(&format!(
            "Config key '{}' would be set to '{}'. Runtime config mutation not yet supported; \
             edit ~/.config/namimado/namimado.yaml instead.",
            req.key, req.value
        )))
    }

    // -- App-specific tools --

    #[tool(description = "Open a new tab, optionally with a URL.")]
    async fn new_tab(
        &self,
        Parameters(req): Parameters<NewTabRequest>,
    ) -> Result<CallToolResult, McpError> {
        let url = req.url.unwrap_or_else(|| self.config.homepage.clone());
        // Tab creation requires the running app; report what would happen.
        Ok(ToolResponse::success(&serde_json::json!({
            "action": "new_tab",
            "url": url,
            "note": "Tab operations require a running browser instance. This tool sends the command to the browser.",
        })))
    }

    #[tool(description = "Close a tab by index. Defaults to the active tab.")]
    async fn close_tab(
        &self,
        Parameters(req): Parameters<CloseTabRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(&serde_json::json!({
            "action": "close_tab",
            "index": req.index,
            "note": "Tab operations require a running browser instance.",
        })))
    }

    #[tool(
        description = "Navigate to a URL and run the full nami-core Lisp substrate \
                       pipeline (framework detect → route match → query dispatch → \
                       derived-aware effects → agent decisions → component + alias \
                       expansion → transform apply). Returns the structured report — \
                       byte-identical with POST /navigate on the HTTP surface."
    )]
    async fn navigate(
        &self,
        Parameters(req): Parameters<NavigateRequest>,
    ) -> Result<CallToolResult, McpError> {
        let svc = self.service.clone();
        let url = req.url.clone();
        // Hop onto blocking pool — SubstratePipeline uses reqwest::blocking
        // internally; it panics inside a tokio async context.
        let result = tokio::task::spawn_blocking(move || {
            svc.navigate(crate::api::NavigateRequest { url })
        })
        .await;
        match result {
            Ok(Ok(resp)) => Ok(ToolResponse::success(
                &serde_json::to_value(&resp).unwrap_or_default(),
            )),
            Ok(Err(e)) => Ok(ToolResponse::error(&format!("navigate_failed: {e}"))),
            Err(e) => Ok(ToolResponse::error(&format!("join_error: {e}"))),
        }
    }

    #[tool(
        description = "Fetch the structured substrate report from the most recent \
                       navigate. Returns state cells, derived values, effects fired, \
                       agents fired, transforms applied, detected frameworks. Same \
                       payload as GET /report on the HTTP surface."
    )]
    async fn get_last_report(&self) -> Result<CallToolResult, McpError> {
        match self.service.last_report() {
            Some(r) => Ok(ToolResponse::success(
                &serde_json::to_value(&r).unwrap_or_default(),
            )),
            None => Ok(ToolResponse::error(
                "no_navigate_yet: call the navigate tool first",
            )),
        }
    }

    #[tool(
        description = "Current Lisp substrate state-store snapshot. Every (defstate …) \
                       cell's current value, accumulating across every navigate. Same \
                       payload as GET /state on the HTTP surface."
    )]
    async fn get_state(&self) -> Result<CallToolResult, McpError> {
        let cells = self.service.state_snapshot();
        Ok(ToolResponse::success(
            &serde_json::to_value(&cells).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Self-describing typescape — the arch-synthesizer leaf \
                       manifest for nami. Every DSL keyword, AST domain, \
                       canonical n-* tag, WASM host API, shipped normalize \
                       pack, HTTP endpoint, MCP tool. BLAKE3-attested. Use \
                       this instead of scraping source code to understand \
                       what the binary does. Same payload as GET /typescape."
    )]
    async fn get_typescape(&self) -> Result<CallToolResult, McpError> {
        let ts = crate::typescape::typescape();
        Ok(ToolResponse::success(
            &serde_json::to_value(&ts).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Re-scan ~/.config/namimado/{extensions,transforms,aliases}.lisp \
                       + substrate.d/*.lisp and swap in a fresh pipeline. State \
                       store resets to the new (defstate) seeds. Use after editing \
                       rule packs. Returns the freshly loaded inventory so you \
                       don't need a second round-trip. Same as POST /reload."
    )]
    async fn reload(&self) -> Result<CallToolResult, McpError> {
        let svc = self.service.clone();
        let resp = tokio::task::spawn_blocking(move || svc.reload())
            .await
            .map_err(|e| McpError::internal_error(format!("join_error: {e}"), None))?;
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Inventory of every DSL form currently loaded by the Lisp \
                       substrate — states, effects, predicates, plans, agents, \
                       routes, queries, derived, components, normalize_rules, \
                       transforms, aliases. One array of names per DSL. Same \
                       payload as GET /rules."
    )]
    async fn get_rules(&self) -> Result<CallToolResult, McpError> {
        let inv = self.service.rules_inventory();
        Ok(ToolResponse::success(
            &serde_json::to_value(&inv).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "The last navigated page absorbed into Lisp space — full DOM \
                       rendered as S-expressions, depth-capped at 8 levels. This is \
                       what `(defdom-transform …)`, `(defscrape …)`, and agents \
                       reason over. Same payload as GET /dom."
    )]
    async fn get_dom_sexp(&self) -> Result<CallToolResult, McpError> {
        match self.service.last_dom_sexp() {
            Some(sexp) => Ok(ToolResponse::text(&sexp)),
            None => Ok(ToolResponse::error(
                "no_navigate_yet: call the navigate tool first",
            )),
        }
    }

    #[tool(description = "List all open tabs with their URLs and titles.")]
    async fn list_tabs(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(&serde_json::json!({
            "action": "list_tabs",
            "note": "Tab listing requires a running browser instance.",
        })))
    }

    #[tool(description = "Get bookmarks, optionally filtered by a search query.")]
    async fn get_bookmarks(
        &self,
        Parameters(req): Parameters<GetBookmarksRequest>,
    ) -> Result<CallToolResult, McpError> {
        let all = self.service.bookmarks_list();
        let filtered = if let Some(q) = req.query.as_deref().filter(|s| !s.is_empty()) {
            let q = q.to_ascii_lowercase();
            all.into_iter()
                .filter(|b| {
                    b.title.to_ascii_lowercase().contains(&q)
                        || b.url.to_ascii_lowercase().contains(&q)
                        || b.tags.iter().any(|t| t.to_ascii_lowercase().contains(&q))
                })
                .collect::<Vec<_>>()
        } else {
            all
        };
        Ok(ToolResponse::success(
            &serde_json::to_value(&filtered).unwrap_or_default(),
        ))
    }

    #[tool(description = "Add a URL to bookmarks. Returns { added: true } if new, false if already bookmarked.")]
    async fn add_bookmark(
        &self,
        Parameters(req): Parameters<AddBookmarkRequest>,
    ) -> Result<CallToolResult, McpError> {
        let Some(url) = req.url else {
            return Ok(ToolResponse::error("url is required"));
        };
        let api_req = crate::api::AddBookmarkRequest {
            url,
            title: req.title,
            folder: None,
            tags: req.tags.unwrap_or_default(),
        };
        match self.service.bookmark_add(api_req) {
            Ok(added) => Ok(ToolResponse::success(
                &serde_json::json!({ "added": added }),
            )),
            Err(e) => Ok(ToolResponse::error(&format!("bookmark_add_failed: {e}"))),
        }
    }

    #[tool(
        description = "Accessibility tree of the last navigated page — ARIA-shaped. \
                       Canonical n-* vocab IS the role map, so every framework \
                       normalize-pack covers gives you free a11y. Same payload as \
                       GET /accessibility."
    )]
    async fn get_accessibility_tree(&self) -> Result<CallToolResult, McpError> {
        match self.service.last_accessibility_tree() {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(
                "no_navigate_yet: call the navigate tool first",
            )),
        }
    }

    #[tool(
        description = "Verify a signed-extension envelope against the trust \
                       DB. Body is a full SignedExtension JSON (spec + \
                       ed25519 signature). Returns status: trusted / \
                       valid-but-untrusted / invalid. Use before install \
                       to refuse untrusted bundles in strict mode."
    )]
    async fn verify_extension(
        &self,
        Parameters(req): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let signed: nami_core::extension::SignedExtension = match serde_json::from_value(req) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResponse::error(&format!("bad_body: {e}"))),
        };
        let resp = self.service.verify_signed_extension(&signed);
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every trusted ed25519 pubkey (base64) in the trust DB.")]
    async fn trustdb_list(&self) -> Result<CallToolResult, McpError> {
        let keys = self.service.trustdb_keys();
        Ok(ToolResponse::success(
            &serde_json::to_value(&keys).unwrap_or_default(),
        ))
    }

    #[tool(description = "Add a base64-encoded ed25519 pubkey to the trust DB.")]
    async fn trustdb_add(
        &self,
        Parameters(req): Parameters<TrustPubkeyRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::TrustdbKeyRequest {
            public_key: req.public_key.clone(),
            signed_by: req.signed_by,
        };
        if self.service.trust_pubkey(api_req) {
            Ok(ToolResponse::success(&serde_json::json!({
                "trusted": req.public_key,
            })))
        } else {
            Ok(ToolResponse::error("trustdb_locked"))
        }
    }

    #[tool(description = "Revoke a previously trusted pubkey.")]
    async fn trustdb_revoke(
        &self,
        Parameters(req): Parameters<RevokePubkeyRequest>,
    ) -> Result<CallToolResult, McpError> {
        if self.service.revoke_pubkey(&req.public_key) {
            Ok(ToolResponse::success(&serde_json::json!({
                "revoked": req.public_key,
            })))
        } else {
            Ok(ToolResponse::error(&format!(
                "trustdb_key_missing: {}",
                req.public_key
            )))
        }
    }

    #[tool(
        description = "Unified URL-bar autocomplete — run the (defomnibox) \
                       ranker against a query. Returns suggestions across \
                       history, bookmarks, commands, tabs, extensions, \
                       search providers, and direct URLs. Same payload as \
                       GET /omnibox?q=…"
    )]
    async fn omnibox(
        &self,
        Parameters(req): Parameters<OmniboxToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let resp = self.service.omnibox(&req.q, req.profile.as_deref());
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "List every (defcommand) with the bound chords that \
                       currently invoke it. Same payload as GET /commands. \
                       Use to introspect the Vim-mode surface the user has \
                       authored (plus any shipped pack)."
    )]
    async fn commands_list(&self) -> Result<CallToolResult, McpError> {
        let list = self.service.commands_list();
        Ok(ToolResponse::success(
            &serde_json::to_value(&list).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Dispatch a typed key sequence against the (defbind) \
                       registry in a given mode. Returns run/prefix/miss. \
                       Used to test bindings, prototype sequences, or drive \
                       the browser from an MCP client without touching the \
                       GPU key pipeline. Same as POST /commands/dispatch."
    )]
    async fn dispatch_key(
        &self,
        Parameters(req): Parameters<DispatchKeyToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::DispatchKeyRequest {
            typed: req.typed,
            mode: req.mode,
        };
        let resp = self.service.dispatch_key(api_req);
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Reader-view simplification of the last navigated page. \
                       Applies a (defreader) profile (host-matching by default, \
                       or named via :name) and returns title, byline, word \
                       count, plain text render, and simplified HTML. \
                       Absorbs Firefox Reader View + Safari Reader."
    )]
    async fn reader(
        &self,
        Parameters(req): Parameters<ReaderRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.reader(req.name.as_deref()) {
            Some(r) => Ok(ToolResponse::success(
                &serde_json::to_value(&r).unwrap_or_default(),
            )),
            None => Ok(ToolResponse::error(
                "reader_unavailable: no navigate yet, or no reader profile matched",
            )),
        }
    }

    #[tool(
        description = "List installed (defextension) bundles — name, version, \
                       enabled state, permission counts. Same as GET /extensions."
    )]
    async fn extensions_list(&self) -> Result<CallToolResult, McpError> {
        let list = self.service.extensions_list();
        Ok(ToolResponse::success(
            &serde_json::to_value(&list).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full ExtensionSpec for one installed extension.")]
    async fn extension_get(
        &self,
        Parameters(req): Parameters<ExtensionNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.extension_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "extension_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(
        description = "Install a (defextension) bundle from raw tatara-lisp \
                       source. Returns { installed, content_hash }. The hash \
                       changes on any subsequent mutation of the extension set \
                       — BLAKE3-attestable across the decentralized store."
    )]
    async fn extension_install(
        &self,
        Parameters(req): Parameters<ExtensionInstallToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::ExtensionInstallRequest {
            lisp_source: req.lisp_source,
        };
        match self.service.extension_install(api_req) {
            Ok(r) => Ok(ToolResponse::success(
                &serde_json::to_value(&r).unwrap_or_default(),
            )),
            Err(e) => Ok(ToolResponse::error(&format!(
                "extension_install_failed: {e}"
            ))),
        }
    }

    #[tool(description = "Enable or disable an installed extension at runtime.")]
    async fn extension_set_enabled(
        &self,
        Parameters(req): Parameters<ExtensionToggleToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::ExtensionToggleRequest {
            enabled: req.enabled,
        };
        if self.service.extension_set_enabled(&req.name, api_req) {
            Ok(ToolResponse::success(&serde_json::json!({
                "name": req.name,
                "enabled": req.enabled,
            })))
        } else {
            Ok(ToolResponse::error(&format!(
                "extension_unknown: {}",
                req.name
            )))
        }
    }

    #[tool(description = "Uninstall an extension by name.")]
    async fn extension_remove(
        &self,
        Parameters(req): Parameters<ExtensionNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        if self.service.extension_remove(&req.name) {
            Ok(ToolResponse::success(&serde_json::json!({
                "removed": req.name,
            })))
        } else {
            Ok(ToolResponse::error(&format!(
                "extension_unknown: {}",
                req.name
            )))
        }
    }

    #[tool(
        description = "List every (defstorage …) declared store with its current \
                       entry count. Each store is a pure tatara-lisp append-only \
                       event log, replayed on startup into a live in-memory map. \
                       Same payload as GET /storage."
    )]
    async fn storage_list(&self) -> Result<CallToolResult, McpError> {
        let summaries = self.service.storage_list();
        Ok(ToolResponse::success(
            &serde_json::to_value(&summaries).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Full key→value snapshot of one (defstorage …) store. \
                       Same payload as GET /storage/:name."
    )]
    async fn storage_entries(
        &self,
        Parameters(req): Parameters<StorageStoreRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.storage_entries(&req.store) {
            Some(entries) => Ok(ToolResponse::success(
                &serde_json::to_value(&entries).unwrap_or_default(),
            )),
            None => Ok(ToolResponse::error(&format!(
                "storage_unknown: {}",
                req.store
            ))),
        }
    }

    #[tool(
        description = "Read one key from a (defstorage …) store. Returns the raw \
                       JSON value; Lisp-tagged values round-trip as \
                       `{\"_lisp\": …}`. Same as GET /storage/:name?key=…"
    )]
    async fn storage_get(
        &self,
        Parameters(req): Parameters<StorageKeyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.storage_get(&req.store, &req.key) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "storage_key_missing: {}:{}",
                req.store, req.key
            ))),
        }
    }

    #[tool(
        description = "Write one key→value into a (defstorage …) store. Persisted \
                       to the append-only event log; TTL and compaction apply \
                       per the store's spec. Same as POST /storage/:name."
    )]
    async fn storage_set(
        &self,
        Parameters(req): Parameters<StorageSetToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::StorageSetRequest {
            key: req.key.clone(),
            value: req.value.clone(),
        };
        if self.service.storage_set(&req.store, api_req) {
            Ok(ToolResponse::success(&serde_json::json!({
                "store": req.store,
                "key": req.key,
                "value": req.value,
            })))
        } else {
            Ok(ToolResponse::error(&format!(
                "storage_unknown: {}",
                req.store
            )))
        }
    }

    #[tool(
        description = "Delete one key from a (defstorage …) store. Same as \
                       DELETE /storage/:name?key=…"
    )]
    async fn storage_delete(
        &self,
        Parameters(req): Parameters<StorageKeyRequest>,
    ) -> Result<CallToolResult, McpError> {
        if self.service.storage_delete(&req.store, &req.key) {
            Ok(ToolResponse::success(&serde_json::json!({
                "deleted": true,
                "store": req.store,
                "key": req.key,
            })))
        } else {
            Ok(ToolResponse::error(&format!(
                "storage_key_missing: {}:{}",
                req.store, req.key
            )))
        }
    }

    #[tool(description = "Most recent browsing history entries, newest first. Records a visit on every successful navigate automatically.")]
    async fn history_recent(&self) -> Result<CallToolResult, McpError> {
        let entries = self.service.history_recent(50);
        Ok(ToolResponse::success(
            &serde_json::to_value(&entries).unwrap_or_default(),
        ))
    }

    #[tool(description = "Search browsing history by title or URL substring.")]
    async fn history_search(
        &self,
        Parameters(req): Parameters<HistorySearchRequest>,
    ) -> Result<CallToolResult, McpError> {
        let entries = self.service.history_search(&req.query);
        Ok(ToolResponse::success(
            &serde_json::to_value(&entries).unwrap_or_default(),
        ))
    }
}

// ---------------------------------------------------------------------------
// ServerHandler
// ---------------------------------------------------------------------------

#[tool_handler]
impl ServerHandler for NamimadoMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: rmcp::model::Implementation {
                name: "namimado".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                description: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Namimado desktop browser MCP server. Manage tabs, navigate pages, \
                 and control bookmarks."
                    .to_string(),
            ),
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the MCP server on stdio.
pub async fn run(config: NamimadoConfig) -> Result<(), Box<dyn std::error::Error>> {
    use rmcp::{transport::stdio, ServiceExt};

    let service = NamimadoMcpServer::new(config);
    let server = service.serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
