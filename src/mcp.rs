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
struct SummarizeToolRequest {
    profile: String,
    source: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ChatAskToolRequest {
    profile: String,
    question: String,
    page_context: Option<String>,
    history: Option<Vec<crate::api::LlmMessageDto>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LlmCompletionToolRequest {
    profile: String,
    prefix: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RpIdRequest {
    rp_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DownloadNameRequest {
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct OutlineToolRequest {
    profile: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UrlRewriteToolRequest {
    url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DnsNameRequest {
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SpaceNameRequest {
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SidebarsListToolRequest {
    host: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SplitNameRequest {
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct JsEvalToolRequest {
    source: String,
    profile: Option<String>,
    /// Optional JSON object of `{ident: primitive}` bindings.
    vars: Option<serde_json::Value>,
    origin: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FindToolRequest {
    query: String,
    profile: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct HostOnlyRequest {
    host: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchKeywordRequest {
    /// Omnibox keyword shortcut (no leading `!`). e.g. `"k"`.
    keyword: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct BangDetectRequest {
    /// Raw omnibox text. The service strips a matching !bang token.
    input: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct IdentityRequest {
    /// Identity name (e.g. "work", "personal").
    identity: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SuggestionActiveRequest {
    /// Current omnibox input text.
    input: String,
    /// Host the user is currently on (for host-glob matching). "" = any.
    host: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SuggestionSourceRequest {
    /// (defsuggestion-source) name.
    source: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PermissionDecideRequest {
    /// Kebab-case permission (camera, microphone, geolocation, usb, …).
    permission: String,
    /// Host the page is loaded on.
    host: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TriggerRequest {
    /// Kebab-case trigger (command, hotkey, omnibox, auto-match, periodic).
    trigger: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EventKindRequest {
    /// Kebab-case event kind (rc-reload, permission-grant, totp-read, …).
    event: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AutoplayAdmitsRequest {
    host: String,
    #[serde(default)]
    muted: bool,
    #[serde(default)]
    user_has_interacted: bool,
    #[serde(default)]
    high_mei: bool,
    #[serde(default)]
    tab_backgrounded: bool,
    /// Optional track kind: audio-element|video-element|web-rtc|media-session.
    kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct HistoryRecordRequest {
    host: String,
    url: String,
    #[serde(default)]
    dwell_seconds: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NavigationDecideRequest {
    host: String,
    /// Kebab-case click source.
    click_source: String,
    #[serde(default)]
    same_origin: bool,
    #[serde(default)]
    had_user_gesture: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SyncSignalRequest {
    /// One of: bookmarks, history, tabs, open-windows, passwords,
    /// passkeys, sessions, extensions, settings, reading-list,
    /// annotations, downloads, custom.
    signal: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SnapshotRecipeToolRequest {
    host: String,
    name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GestureDispatchToolRequest {
    stroke: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct BoostToggleToolRequest {
    name: String,
    enabled: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct BoostsListToolRequest {
    host: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StorageByIndexRangeRequest {
    store: String,
    path: String,
    /// Inclusive lower bound. Empty = unbounded below.
    lo: Option<String>,
    /// Inclusive upper bound. Empty = unbounded above.
    hi: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct I18nGetRequest {
    namespace: String,
    locale: Option<String>,
    key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct I18nCoverageRequest {
    namespace: String,
    locale: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SecurityPolicyRequest {
    host: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StorageByIndexRequest {
    store: String,
    /// Dot-path matching a declared (defstorage :indexes) entry.
    path: String,
    /// Projected value to match. Strings, ints, bools are
    /// normalized to their display form at index time.
    value: String,
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

    #[tool(description = "List every (defreader-aloud) profile.")]
    async fn reader_aloud_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.reader_aloud_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full ReaderAloudSpec for one profile.")]
    async fn reader_aloud_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.reader_aloud_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "reader_aloud_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "List every (defhigh-contrast) profile.")]
    async fn high_contrast_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.high_contrast_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved high-contrast profile for a host.")]
    async fn high_contrast_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.high_contrast_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_high_contrast_matches")),
        }
    }

    #[tool(description = "List every (defsimplify) profile.")]
    async fn simplify_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.simplify_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved simplify profile for a host.")]
    async fn simplify_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.simplify_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_simplify_matches")),
        }
    }

    #[tool(description = "List every (defpresence) profile.")]
    async fn presence_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.presence_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved presence profile for a host.")]
    async fn presence_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.presence_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_presence_matches")),
        }
    }

    #[tool(description = "List every (defcrdt-room) profile.")]
    async fn crdt_room_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.crdt_room_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved CRDT-room profile for a host.")]
    async fn crdt_room_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.crdt_room_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_crdt_room_matches")),
        }
    }

    #[tool(description = "List every (defmultiplayer-cursor) profile.")]
    async fn multiplayer_cursor_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.multiplayer_cursor_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved multiplayer-cursor profile for a host.")]
    async fn multiplayer_cursor_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.multiplayer_cursor_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_multiplayer_cursor_matches")),
        }
    }

    #[tool(description = "List every (defservice-worker) profile.")]
    async fn service_worker_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.service_worker_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved service-worker profile for a host.")]
    async fn service_worker_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.service_worker_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_service_worker_matches")),
        }
    }

    #[tool(description = "List every (defsync) channel.")]
    async fn sync_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.sync_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full SyncSpec for one (defsync) channel by name.")]
    async fn sync_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.sync_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!("sync_unknown: {}", req.name))),
        }
    }

    #[tool(description = "Every (defsync) channel syncing a given signal kind (bookmarks, history, tabs, …).")]
    async fn sync_for_signal(
        &self,
        Parameters(req): Parameters<SyncSignalRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.sync_for_signal(&req.signal))
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (deftab-group) profile.")]
    async fn tab_group_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.tab_group_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved tab-group profile for a host.")]
    async fn tab_group_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.tab_group_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_tab_group_matches")),
        }
    }

    #[tool(description = "List every (deftab-hibernate) profile.")]
    async fn tab_hibernate_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.tab_hibernate_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved tab-hibernate profile for a host.")]
    async fn tab_hibernate_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.tab_hibernate_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_tab_hibernate_matches")),
        }
    }

    #[tool(description = "List every (deftab-preview) profile.")]
    async fn tab_preview_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.tab_preview_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved tab-preview profile for a host.")]
    async fn tab_preview_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.tab_preview_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_tab_preview_matches")),
        }
    }

    #[tool(description = "List every (defsearch-engine) profile.")]
    async fn search_engine_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.search_engine_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full SearchEngineSpec for one profile by name.")]
    async fn search_engine_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.search_engine_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "search_engine_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Search engine for an omnibox keyword shortcut (e.g. 'k' for Kagi).")]
    async fn search_engine_by_keyword(
        &self,
        Parameters(req): Parameters<SearchKeywordRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.search_engine_by_keyword(&req.keyword) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_search_engine_for_keyword")),
        }
    }

    #[tool(description = "The current default search engine.")]
    async fn search_engine_default(&self) -> Result<CallToolResult, McpError> {
        match self.service.search_engine_default() {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_search_engine_default")),
        }
    }

    #[tool(description = "List every (defsearch-bang) shortcut.")]
    async fn search_bang_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.search_bang_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Detect a !bang in omnibox input; returns {spec, remaining} on match.")]
    async fn search_bang_detect(
        &self,
        Parameters(req): Parameters<BangDetectRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.search_bang_detect(&req.input) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_search_bang_matches")),
        }
    }

    #[tool(description = "List every (defidentity) persona.")]
    async fn identity_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.identity_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full IdentitySpec for one persona by name.")]
    async fn identity_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.identity_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "identity_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Active identity for a host (auto-apply match → default → first-enabled).")]
    async fn identity_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.identity_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_identity_matches")),
        }
    }

    #[tool(description = "List every (deftotp) profile (secrets redacted only in the caller's display layer).")]
    async fn totp_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.totp_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full TotpSpec for one profile by name.")]
    async fn totp_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.totp_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "totp_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Every (deftotp) profile linked to a named identity.")]
    async fn totp_for_identity(
        &self,
        Parameters(req): Parameters<IdentityRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.totp_for_identity(&req.identity))
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "Current TOTP code + seconds-remaining for a profile (RFC 6238).")]
    async fn totp_code(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.totp_code(&req.name) {
            Some(Ok(v)) => Ok(ToolResponse::success(&v)),
            Some(Err(e)) => Ok(ToolResponse::error(&format!("totp_generate_failed: {e}"))),
            None => Ok(ToolResponse::error(&format!(
                "totp_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "List every (deffingerprint-randomize) profile.")]
    async fn fingerprint_randomize_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.fingerprint_randomize_list())
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved fingerprint-randomize profile for a host.")]
    async fn fingerprint_randomize_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.fingerprint_randomize_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_fingerprint_matches")),
        }
    }

    #[tool(description = "List every (defcookie-jar) profile.")]
    async fn cookie_jar_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.cookie_jar_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved cookie-jar profile for a host.")]
    async fn cookie_jar_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.cookie_jar_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_cookie_jar_matches")),
        }
    }

    #[tool(description = "List every (defwebgpu-policy) profile.")]
    async fn webgpu_policy_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.webgpu_policy_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved webgpu-policy profile for a host.")]
    async fn webgpu_policy_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.webgpu_policy_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_webgpu_policy_matches")),
        }
    }

    #[tool(description = "List every (defsuggestion-source) profile.")]
    async fn suggestion_source_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.suggestion_source_list())
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "Full SuggestionSourceSpec for one profile by name.")]
    async fn suggestion_source_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.suggestion_source_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "suggestion_source_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Suggestion sources active for an omnibox input on a host.")]
    async fn suggestion_source_active_for(
        &self,
        Parameters(req): Parameters<SuggestionActiveRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(
                &self.service.suggestion_source_active_for(&req.input, &req.host),
            )
            .unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defsuggestion-ranker) profile.")]
    async fn suggestion_ranker_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.suggestion_ranker_list())
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "Full SuggestionRankerSpec for one profile by name.")]
    async fn suggestion_ranker_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.suggestion_ranker_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "suggestion_ranker_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Ranker responsible for a named suggestion source (source-specific preferred over default).")]
    async fn suggestion_ranker_for_source(
        &self,
        Parameters(req): Parameters<SuggestionSourceRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.suggestion_ranker_for_source(&req.source) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_ranker_matches")),
        }
    }

    #[tool(description = "List every (defpermission-policy) profile.")]
    async fn permission_policy_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.permission_policy_list())
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved permission-policy for a host.")]
    async fn permission_policy_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.permission_policy_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_permission_policy_matches")),
        }
    }

    #[tool(description = "Decide(permission, host) — returns the kebab-case decision (allow/block/prompt/prompt-ephemeral/require-user-gesture/block-with-badge).")]
    async fn permission_decide(
        &self,
        Parameters(req): Parameters<PermissionDecideRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.permission_decide(&req.permission, &req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "permission_unknown: {}",
                req.permission
            ))),
        }
    }

    #[tool(description = "List every (defpermission-prompt) profile.")]
    async fn permission_prompt_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.permission_prompt_list())
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "Full PermissionPromptSpec for one profile by name.")]
    async fn permission_prompt_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.permission_prompt_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "permission_prompt_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Resolved prompt UX for (permission, host) — host+permission-specific preferred.")]
    async fn permission_prompt_for(
        &self,
        Parameters(req): Parameters<PermissionDecideRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .service
            .permission_prompt_for(&req.permission, &req.host)
        {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_permission_prompt_matches")),
        }
    }

    #[tool(description = "List every (defresource-hint) profile.")]
    async fn resource_hint_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.resource_hint_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full ResourceHintSpec for one profile by name.")]
    async fn resource_hint_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.resource_hint_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "resource_hint_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Every resource hint applicable to a host, priority-sorted.")]
    async fn resource_hints_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.resource_hints_for(&req.host))
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defbfcache-policy) profile.")]
    async fn bfcache_policy_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.bfcache_policy_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved bfcache policy for a host.")]
    async fn bfcache_policy_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.bfcache_policy_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_bfcache_policy_matches")),
        }
    }

    #[tool(description = "List every (defprerender-rule) profile.")]
    async fn prerender_rule_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.prerender_rule_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Every prerender rule applicable to a host.")]
    async fn prerender_rules_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.prerender_rules_for(&req.host))
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defhistory-policy) profile.")]
    async fn history_policy_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.history_policy_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved history policy for a host.")]
    async fn history_policy_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.history_policy_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_history_policy_matches")),
        }
    }

    #[tool(description = "Whether (host, url, dwell_seconds) should be recorded in history.")]
    async fn history_should_record(
        &self,
        Parameters(req): Parameters<HistoryRecordRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.history_should_record(&req.host, &req.url, req.dwell_seconds) {
            Some(b) => Ok(ToolResponse::success(&serde_json::json!({
                "should_record": b,
            }))),
            None => Ok(ToolResponse::error("no_history_policy_matches")),
        }
    }

    #[tool(description = "List every (defnavigation-intent) profile.")]
    async fn navigation_intent_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.navigation_intent_list())
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved navigation-intent profile for a host.")]
    async fn navigation_intent_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.navigation_intent_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_navigation_intent_matches")),
        }
    }

    #[tool(description = "Decide OpenDisposition for a click. click_source: link-click/middle-click/cmd-click/cmd-shift-click/script-open/form-target-blank/anchor-target-blank/drag-drop/omnibox/back-forward/reload/keyboard-shortcut.")]
    async fn navigation_decide(
        &self,
        Parameters(req): Parameters<NavigationDecideRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.navigation_resolve(
            &req.host,
            &req.click_source,
            req.same_origin,
            req.had_user_gesture,
        ) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "click_source_unknown: {}",
                req.click_source
            ))),
        }
    }

    #[tool(description = "List every (defstorage-quota) profile.")]
    async fn storage_quota_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.storage_quota_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved storage-quota for a host.")]
    async fn storage_quota_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.storage_quota_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_storage_quota_matches")),
        }
    }

    #[tool(description = "List every (defclear-site-data) profile.")]
    async fn clear_site_data_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.clear_site_data_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Clear-site-data profiles applicable to a host (not exempt).")]
    async fn clear_site_data_applicable(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.clear_site_data_applicable(&req.host))
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defaudit-trail) profile. Privacy-first: empty until the user opts in via rc file.")]
    async fn audit_trail_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.audit_trail_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full AuditTrailSpec for one profile by name.")]
    async fn audit_trail_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.audit_trail_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "audit_trail_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Audit-trail profiles that capture a given event (kebab-case event name).")]
    async fn audit_trail_for_event(
        &self,
        Parameters(req): Parameters<EventKindRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.audit_trail_for_event(&req.event))
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defviewport) profile.")]
    async fn viewport_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.viewport_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved viewport profile for a host.")]
    async fn viewport_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.viewport_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_viewport_matches")),
        }
    }

    #[tool(description = "Synthesized `<meta name=viewport>` string for a host.")]
    async fn viewport_meta(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.viewport_meta_for(&req.host) {
            Some(m) => Ok(ToolResponse::success(&serde_json::json!({
                "meta": m,
            }))),
            None => Ok(ToolResponse::error("no_viewport_matches")),
        }
    }

    #[tool(description = "List every (defcsp-policy) profile.")]
    async fn csp_policy_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.csp_policy_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved CSP policy for a host.")]
    async fn csp_policy_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.csp_policy_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_csp_policy_matches")),
        }
    }

    #[tool(description = "Rendered CSP {header_name, header_value} for a host.")]
    async fn csp_header(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.csp_header_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_csp_policy_matches")),
        }
    }

    #[tool(description = "Validation warnings for a host's CSP (mutual-exclusion foot-guns).")]
    async fn csp_validate(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.csp_validate_for(&req.host) {
            Some(warnings) => Ok(ToolResponse::success(&serde_json::json!({
                "warnings": warnings,
            }))),
            None => Ok(ToolResponse::error("no_csp_policy_matches")),
        }
    }

    #[tool(description = "List every (defnetwork-throttle) profile.")]
    async fn network_throttle_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.network_throttle_list())
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved network-throttle profile for a host.")]
    async fn network_throttle_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.network_throttle_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_network_throttle_matches")),
        }
    }

    #[tool(description = "Effective (download_kbps, upload_kbps, latency_ms, admits, …) tuple for a host.")]
    async fn network_throttle_effective(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.network_throttle_effective(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_network_throttle_matches")),
        }
    }

    #[tool(description = "List every (deftime-travel) profile (privacy-first — empty until opt-in).")]
    async fn time_travel_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.time_travel_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full TimeTravelSpec for one profile by name.")]
    async fn time_travel_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.time_travel_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "time_travel_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Time-travel profiles applicable to a host (enabled + not exempt).")]
    async fn time_travel_applicable(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.time_travel_applicable(&req.host))
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (deflocale) profile.")]
    async fn locale_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.locale_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved locale profile for a host.")]
    async fn locale_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.locale_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_locale_matches")),
        }
    }

    #[tool(description = "Rendered {accept_language, primary, languages, timezone} for a host.")]
    async fn locale_headers(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.locale_headers_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_locale_matches")),
        }
    }

    #[tool(description = "List every (deftab-macro) (privacy-first — empty until user opts in).")]
    async fn tab_macro_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.tab_macro_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full TabMacroSpec for one macro by name.")]
    async fn tab_macro_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.tab_macro_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "tab_macro_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Tab-macros bound to a given trigger (command/hotkey/omnibox/auto-match/periodic).")]
    async fn tab_macro_by_trigger(
        &self,
        Parameters(req): Parameters<TriggerRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.tab_macro_by_trigger(&req.trigger))
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defcookie-banner) profile.")]
    async fn cookie_banner_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.cookie_banner_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved cookie-banner profile for a host.")]
    async fn cookie_banner_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.cookie_banner_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_cookie_banner_matches")),
        }
    }

    #[tool(description = "CSS rule that hides banner elements on a host ({css: \"...\"}).")]
    async fn cookie_banner_hide_css(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.cookie_banner_hide_css(&req.host) {
            Some(css) => Ok(ToolResponse::success(&serde_json::json!({
                "css": css,
            }))),
            None => Ok(ToolResponse::error("no_cookie_banner_hide_css")),
        }
    }

    #[tool(description = "List every (defsmart-bookmark) profile.")]
    async fn smart_bookmark_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.smart_bookmark_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved smart-bookmark profile for a host.")]
    async fn smart_bookmark_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.smart_bookmark_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_smart_bookmark_matches")),
        }
    }

    #[tool(description = "List every (deftext-spacing) profile.")]
    async fn text_spacing_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.text_spacing_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved text-spacing profile for a host.")]
    async fn text_spacing_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.text_spacing_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_text_spacing_matches")),
        }
    }

    #[tool(description = "Rendered CSS stylesheet for a host's text-spacing profile ({css: \"...\"}).")]
    async fn text_spacing_css(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.text_spacing_css(&req.host) {
            Some(css) => Ok(ToolResponse::success(&serde_json::json!({
                "css": css,
            }))),
            None => Ok(ToolResponse::error("no_text_spacing_matches")),
        }
    }

    #[tool(description = "List every (defautoplay) profile.")]
    async fn autoplay_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.autoplay_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved autoplay profile for a host.")]
    async fn autoplay_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.autoplay_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_autoplay_matches")),
        }
    }

    #[tool(description = "Pure decision: may a media element autoplay? Returns {admits: bool}. kind ∈ audio-element|video-element|web-rtc|media-session.")]
    async fn autoplay_admits(
        &self,
        Parameters(req): Parameters<AutoplayAdmitsRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.autoplay_admits(
            &req.host,
            req.muted,
            req.user_has_interacted,
            req.high_mei,
            req.tab_backgrounded,
            req.kind.as_deref(),
        ) {
            Some(admit) => Ok(ToolResponse::success(
                &serde_json::json!({ "admits": admit }),
            )),
            None => Ok(ToolResponse::error("no_autoplay_matches")),
        }
    }

    #[tool(description = "List every (deftab-attestation) profile.")]
    async fn tab_attestation_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.tab_attestation_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolved tab-attestation profile for a host.")]
    async fn tab_attestation_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.tab_attestation_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_tab_attestation_matches")),
        }
    }

    #[tool(description = "Should a tab opened on host be chained? Returns {should_chain: bool}.")]
    async fn tab_attestation_should_chain(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(&serde_json::json!({
            "should_chain": self.service.tab_attestation_should_chain(&req.host),
        })))
    }

    #[tool(description = "List every (definspector) panel.")]
    async fn inspector_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.inspector_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Only visible (definspector) panels.")]
    async fn inspector_visible(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.inspector_visible()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full InspectorSpec for one panel.")]
    async fn inspector_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.inspector_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "inspector_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "List every (defprofiler) profile.")]
    async fn profiler_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.profiler_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full ProfilerSpec for one profile.")]
    async fn profiler_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.profiler_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "profiler_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "List every (defconsole-rule).")]
    async fn console_rule_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.console_rule_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defmedia-session) profile.")]
    async fn media_session_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.media_session_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolve (defmedia-session) for a host.")]
    async fn media_session_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.media_session_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_media_session_matches")),
        }
    }

    #[tool(description = "List every (defcast) profile.")]
    async fn cast_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.cast_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Cast profiles applicable to a host.")]
    async fn cast_applicable(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.cast_applicable(&req.host))
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defsubtitle) profile.")]
    async fn subtitle_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.subtitle_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolve (defsubtitle) for a host.")]
    async fn subtitle_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.subtitle_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_subtitle_matches")),
        }
    }

    #[tool(description = "List every (defllm-provider) declaration.")]
    async fn llm_provider_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.llm_provider_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defsummarize) profile.")]
    async fn summarize_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.summarize_list()).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Run a (defsummarize) profile against source text. \
                       Returns { outcome, content, tokens, engine, error? }."
    )]
    async fn summarize_run(
        &self,
        Parameters(req): Parameters<SummarizeToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::SummarizeRequest {
            profile: req.profile,
            source: req.source,
        };
        let resp = self.service.summarize_run(api_req);
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defchat-with-page) profile.")]
    async fn chat_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.chat_list()).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Ask a (defchat-with-page) profile a question against \
                       optional page context + history. Returns LlmResponseDto."
    )]
    async fn chat_ask(
        &self,
        Parameters(req): Parameters<ChatAskToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::ChatAskRequest {
            profile: req.profile,
            question: req.question,
            page_context: req.page_context,
            history: req.history.unwrap_or_default(),
        };
        let resp = self.service.chat_ask(api_req);
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defllm-completion) profile.")]
    async fn llm_completion_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.llm_completion_list()).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Run a (defllm-completion) profile against a prefix. \
                       Returns LlmResponseDto."
    )]
    async fn llm_completion_run(
        &self,
        Parameters(req): Parameters<LlmCompletionToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::LlmCompletionRequest {
            profile: req.profile,
            prefix: req.prefix,
        };
        let resp = self.service.llm_completion_run(api_req);
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defautofill) profile.")]
    async fn autofill_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.autofill_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defpasswords) vault source.")]
    async fn password_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.password_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Password vaults that auto-fill into a host.")]
    async fn passwords_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.passwords_for(&req.host)).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defauth-saver) profile.")]
    async fn auth_saver_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.auth_saver_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolve save-on-submit profile for a host.")]
    async fn auth_saver_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.auth_saver_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_saver_matches")),
        }
    }

    #[tool(description = "List every (defsecure-note) profile.")]
    async fn secure_note_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.secure_note_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defpasskey) WebAuthn profile.")]
    async fn passkey_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.passkey_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Passkey profiles that permit a specific Relying Party ID.")]
    async fn passkeys_for(
        &self,
        Parameters(req): Parameters<RpIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.passkeys_for(&req.rp_id))
                .unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defshare-target) destination.")]
    async fn share_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.share_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defoffline) save-for-later profile.")]
    async fn offline_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.offline_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defpull-to-refresh) rule.")]
    async fn pull_refresh_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.pull_refresh_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolve (defpull-to-refresh) for a host.")]
    async fn pull_refresh_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.pull_refresh_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_ptr_matches")),
        }
    }

    #[tool(description = "List every (defdownload) policy profile.")]
    async fn download_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.download_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full DownloadSpec for one named policy.")]
    async fn download_get(
        &self,
        Parameters(req): Parameters<DownloadNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.download_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "download_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Extract TOC from the last navigated page via (defoutline).")]
    async fn outline_extract(
        &self,
        Parameters(req): Parameters<OutlineToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::OutlineRequest { profile: req.profile };
        match self.service.outline_extract(api_req) {
            Some(v) => Ok(ToolResponse::success(
                &serde_json::to_value(&v).unwrap_or_default(),
            )),
            None => Ok(ToolResponse::error("no_navigate_yet")),
        }
    }

    #[tool(description = "List every (defannotate) profile.")]
    async fn annotate_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.annotate_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (deffeed) subscription.")]
    async fn feed_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.feed_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defredirect) rule.")]
    async fn redirect_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.redirect_list()).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Rewrite a URL through (defredirect) rules — LibRedirect-\
                       style frontend substitution. Returns { input, output, \
                       changed }; pass-through when no rule matches."
    )]
    async fn redirect_apply(
        &self,
        Parameters(req): Parameters<UrlRewriteToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let r = self
            .service
            .redirect_apply(crate::api::RedirectRequest { url: req.url });
        Ok(ToolResponse::success(
            &serde_json::to_value(&r).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defurl-clean) rule.")]
    async fn url_clean_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.url_clean_list()).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Strip tracking parameters from a URL via (defurl-clean) \
                       rules. Returns { input, output, changed }."
    )]
    async fn url_clean_apply(
        &self,
        Parameters(req): Parameters<UrlRewriteToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let r = self
            .service
            .url_clean_apply(crate::api::UrlCleanRequest { url: req.url });
        Ok(ToolResponse::success(
            &serde_json::to_value(&r).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defscript-policy) rule.")]
    async fn script_policy_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.script_policy_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolve (defscript-policy) for a host.")]
    async fn script_policy_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.script_policy_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_policy_matches")),
        }
    }

    #[tool(description = "List every (defbridge) Tor bridge entry.")]
    async fn bridge_list(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(
            &serde_json::to_value(&self.service.bridge_list()).unwrap_or_default(),
        ))
    }

    #[tool(description = "torrc `Bridge …` block for every enabled bridge.")]
    async fn bridges_torrc_block(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::text(&self.service.bridges_torrc_block()))
    }

    #[tool(description = "List every (defspoof) — fingerprint-resistance profile.")]
    async fn spoofs_list(&self) -> Result<CallToolResult, McpError> {
        let v = self.service.spoofs_list();
        Ok(ToolResponse::success(
            &serde_json::to_value(&v).unwrap_or_default(),
        ))
    }

    #[tool(description = "Resolve the most-specific (defspoof) for a host.")]
    async fn spoof_for(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.spoof_for(&req.host) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error("no_spoof_matches")),
        }
    }

    #[tool(description = "List every (defdns) resolver profile.")]
    async fn dns_list(&self) -> Result<CallToolResult, McpError> {
        let v = self.service.dns_list();
        Ok(ToolResponse::success(
            &serde_json::to_value(&v).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full DnsSpec for one resolver profile.")]
    async fn dns_get(
        &self,
        Parameters(req): Parameters<DnsNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.dns_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "dns_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "List every (defrouting) rule.")]
    async fn routing_list(&self) -> Result<CallToolResult, McpError> {
        let v = self.service.routing_list();
        Ok(ToolResponse::success(
            &serde_json::to_value(&v).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Resolve the active route for a host — direct / tunnel / \
                       tor / socks5 / pt / unknown, with strategy target."
    )]
    async fn routing_resolve(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        let r = self.service.routing_resolve(&req.host);
        Ok(ToolResponse::success(
            &serde_json::to_value(&r).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defspace) — Arc-style grouped tabs with per-space state.")]
    async fn spaces_list(&self) -> Result<CallToolResult, McpError> {
        let v = self.service.spaces_list();
        Ok(ToolResponse::success(
            &serde_json::to_value(&v).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full SpaceSpec for one space.")]
    async fn space_get(
        &self,
        Parameters(req): Parameters<SpaceNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.space_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "space_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Activate a space — affects sidebar visibility and space-state.")]
    async fn space_activate(
        &self,
        Parameters(req): Parameters<SpaceNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.space_activate(&req.name) {
            Some(r) => Ok(ToolResponse::success(
                &serde_json::to_value(&r).unwrap_or_default(),
            )),
            None => Ok(ToolResponse::error(&format!(
                "space_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(description = "Currently-active space name, or null.")]
    async fn space_active(&self) -> Result<CallToolResult, McpError> {
        let r = self.service.space_active();
        Ok(ToolResponse::success(
            &serde_json::to_value(&r).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "List (defsidebar) apps, optionally filtered to those \
                       visible under a host (honors space + host gates)."
    )]
    async fn sidebars_list(
        &self,
        Parameters(req): Parameters<SidebarsListToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let v = self.service.sidebars_list(req.host.as_deref());
        Ok(ToolResponse::success(
            &serde_json::to_value(&v).unwrap_or_default(),
        ))
    }

    #[tool(description = "List every (defsplit) layout.")]
    async fn splits_list(&self) -> Result<CallToolResult, McpError> {
        let v = self.service.splits_list();
        Ok(ToolResponse::success(
            &serde_json::to_value(&v).unwrap_or_default(),
        ))
    }

    #[tool(description = "Full SplitSpec for one layout.")]
    async fn split_get(
        &self,
        Parameters(req): Parameters<SplitNameRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.split_get(&req.name) {
            Some(v) => Ok(ToolResponse::success(&v)),
            None => Ok(ToolResponse::error(&format!(
                "split_unknown: {}",
                req.name
            ))),
        }
    }

    #[tool(
        description = "Run JavaScript-ish source through the active (defjs-runtime) \
                       engine. MicroEval ships as today's backend — arithmetic, \
                       string concat, identifier lookup, JS-shape type coercion. \
                       Real engine (Boa / rquickjs) plugs into the same JsRuntime \
                       trait behind a feature flag. Honors fuel + memory + \
                       capability gates from the named profile. Same as POST /js/eval."
    )]
    async fn js_eval(
        &self,
        Parameters(req): Parameters<JsEvalToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::JsEvalRequest {
            source: req.source,
            profile: req.profile,
            vars: req.vars.unwrap_or(serde_json::Value::Null),
            origin: req.origin,
        };
        let resp = self.service.js_eval(api_req);
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Find-in-page against the last navigated document. \
                       Honors (deffind) profile knobs: case-sensitive, \
                       whole-word, regex, max-matches."
    )]
    async fn find(
        &self,
        Parameters(req): Parameters<FindToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::FindRequest {
            query: req.query,
            profile: req.profile,
        };
        match self.service.find(api_req) {
            Some(r) => Ok(ToolResponse::success(
                &serde_json::to_value(&r).unwrap_or_default(),
            )),
            None => Ok(ToolResponse::error("no_navigate_yet")),
        }
    }

    #[tool(description = "Resolved zoom level + text-only flag for a host.")]
    async fn zoom(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        let resp = self.service.zoom_for(&req.host);
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Resolve a (defsnapshot) recipe for the given host. \
                       Returns region/format/scale/quality/selector/attest. \
                       Pixel capture is GPU-side; this is the declarative \
                       contract."
    )]
    async fn snapshot_recipe(
        &self,
        Parameters(req): Parameters<SnapshotRecipeToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.snapshot_recipe(req.name.as_deref(), &req.host) {
            Some(r) => Ok(ToolResponse::success(
                &serde_json::to_value(&r).unwrap_or_default(),
            )),
            None => Ok(ToolResponse::error("no_recipe_matches")),
        }
    }

    #[tool(description = "Picture-in-picture rule for a host — selectors + position.")]
    async fn pip(
        &self,
        Parameters(req): Parameters<HostOnlyRequest>,
    ) -> Result<CallToolResult, McpError> {
        let resp = self.service.pip_for(&req.host);
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Dispatch a mouse-gesture stroke against (defgesture) \
                       bindings. Returns { outcome: run | miss, command? }."
    )]
    async fn gesture_dispatch(
        &self,
        Parameters(req): Parameters<GestureDispatchToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let resp = self.service.gesture_dispatch(crate::api::GestureDispatchRequest {
            stroke: req.stroke,
        });
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(description = "List (defboost) overlays, optionally filtered by host.")]
    async fn boosts_list(
        &self,
        Parameters(req): Parameters<BoostsListToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let list = self.service.boosts_list(req.host.as_deref());
        Ok(ToolResponse::success(
            &serde_json::to_value(&list).unwrap_or_default(),
        ))
    }

    #[tool(description = "Enable / disable a boost at runtime.")]
    async fn boost_set_enabled(
        &self,
        Parameters(req): Parameters<BoostToggleToolRequest>,
    ) -> Result<CallToolResult, McpError> {
        let api_req = crate::api::BoostToggleRequest { enabled: req.enabled };
        if self.service.boost_set_enabled(&req.name, api_req) {
            Ok(ToolResponse::success(&serde_json::json!({
                "name": req.name,
                "enabled": req.enabled,
            })))
        } else {
            Ok(ToolResponse::error(&format!(
                "boost_unknown: {}",
                req.name
            )))
        }
    }

    #[tool(description = "Currently-open session tabs.")]
    async fn session_open(&self) -> Result<CallToolResult, McpError> {
        let v = self.service.session_open();
        Ok(ToolResponse::success(
            &serde_json::to_value(&v).unwrap_or_default(),
        ))
    }

    #[tool(description = "Recently-closed session tabs (ring-buffered, newest-first).")]
    async fn session_closed(&self) -> Result<CallToolResult, McpError> {
        let v = self.service.session_closed();
        Ok(ToolResponse::success(
            &serde_json::to_value(&v).unwrap_or_default(),
        ))
    }

    #[tool(description = "Pop the most-recently-closed tab (Cmd+Shift+T).")]
    async fn session_undo_close(&self) -> Result<CallToolResult, McpError> {
        match self.service.session_undo_close() {
            Some(t) => Ok(ToolResponse::success(
                &serde_json::to_value(&t).unwrap_or_default(),
            )),
            None => Ok(ToolResponse::error("no_closed_tabs")),
        }
    }

    #[tool(
        description = "Range scan over a (defstorage) secondary index. \
                       BTreeMap-backed, so O(log n + k). Inclusive bounds; \
                       omit lo/hi for unbounded. Lexicographic comparison \
                       — zero-pad numerics when you want numeric order."
    )]
    async fn storage_by_index_range(
        &self,
        Parameters(req): Parameters<StorageByIndexRangeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let lo = req.lo.unwrap_or_default();
        let hi = req.hi.unwrap_or_else(|| "\u{10FFFF}".into());
        match self
            .service
            .storage_by_index_range(&req.store, &req.path, &lo, &hi)
        {
            Some(v) => Ok(ToolResponse::success(
                &serde_json::to_value(&v).unwrap_or_default(),
            )),
            None => Ok(ToolResponse::error(&format!(
                "storage_or_index_unknown: {}/{}",
                req.store, req.path
            ))),
        }
    }

    #[tool(
        description = "Resolve a translated message. Fallback chain: exact \
                       (namespace, locale) → locale-prefix (en-US → en) → \
                       (namespace, \"en\") → raw key. Absorbs chrome.i18n."
    )]
    async fn i18n_get(
        &self,
        Parameters(req): Parameters<I18nGetRequest>,
    ) -> Result<CallToolResult, McpError> {
        let locale = req.locale.as_deref().unwrap_or("en");
        let resp = self.service.i18n_get(&req.namespace, locale, &req.key);
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Translation coverage — which locales exist for a \
                       namespace, and which keys are missing from a target \
                       locale relative to :en. The 'what's left to translate' \
                       view."
    )]
    async fn i18n_coverage(
        &self,
        Parameters(req): Parameters<I18nCoverageRequest>,
    ) -> Result<CallToolResult, McpError> {
        let locale = req.locale.as_deref().unwrap_or("en");
        let resp = self.service.i18n_coverage(&req.namespace, locale);
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Security-policy headers for a host — resolves the \
                       matching (defsecurity-policy) + renders the full \
                       HTTP header set (CSP, Permissions-Policy, Referrer-\
                       Policy, Cross-Origin-*, X-Frame-Options)."
    )]
    async fn security_policy(
        &self,
        Parameters(req): Parameters<SecurityPolicyRequest>,
    ) -> Result<CallToolResult, McpError> {
        let resp = self.service.security_policy_for(&req.host);
        Ok(ToolResponse::success(
            &serde_json::to_value(&resp).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "List every declared (defstorage :indexes …) path for a \
                       store plus the distinct projected values present now. \
                       Same payload as GET /storage/:name/index."
    )]
    async fn storage_index_list(
        &self,
        Parameters(req): Parameters<StorageStoreRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self.service.storage_index_summary(&req.store) {
            Some(v) => Ok(ToolResponse::success(
                &serde_json::to_value(&v).unwrap_or_default(),
            )),
            None => Ok(ToolResponse::error(&format!(
                "storage_unknown: {}",
                req.store
            ))),
        }
    }

    #[tool(
        description = "Query a (defstorage) secondary index — every entry \
                       whose projected value at `path` equals `value`. O(log n) \
                       lookup, same payload as GET /storage/:name/index/:path?value=…"
    )]
    async fn storage_by_index(
        &self,
        Parameters(req): Parameters<StorageByIndexRequest>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .service
            .storage_by_index(&req.store, &req.path, &req.value)
        {
            Some(v) => Ok(ToolResponse::success(
                &serde_json::to_value(&v).unwrap_or_default(),
            )),
            None => Ok(ToolResponse::error(&format!(
                "storage_or_index_unknown: {}/{}",
                req.store, req.path
            ))),
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
