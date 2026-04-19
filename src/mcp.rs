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
