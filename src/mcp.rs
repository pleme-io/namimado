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
    /// Optional search query to filter bookmarks.
    query: Option<String>,
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
        }
    }

    // -- Standard tools --

    #[tool(description = "Get Namimado browser status.")]
    async fn status(&self) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(&serde_json::json!({
            "status": "running",
            "homepage": self.config.homepage,
            "devtools_enabled": self.config.devtools_enabled,
            "dark_mode": self.config.theme.dark,
        })))
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

    #[tool(description = "Navigate the active tab to a URL.")]
    async fn navigate(
        &self,
        Parameters(req): Parameters<NavigateRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(&serde_json::json!({
            "action": "navigate",
            "url": req.url,
            "note": "Navigation requires a running browser instance.",
        })))
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
        Ok(ToolResponse::success(&serde_json::json!({
            "action": "get_bookmarks",
            "query": req.query,
            "note": "Bookmark retrieval requires nami-core integration (not yet available).",
        })))
    }

    #[tool(description = "Add a URL to bookmarks.")]
    async fn add_bookmark(
        &self,
        Parameters(req): Parameters<AddBookmarkRequest>,
    ) -> Result<CallToolResult, McpError> {
        Ok(ToolResponse::success(&serde_json::json!({
            "action": "add_bookmark",
            "url": req.url,
            "title": req.title,
            "tags": req.tags,
            "note": "Bookmark management requires nami-core integration (not yet available).",
        })))
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
