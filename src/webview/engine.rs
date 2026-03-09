use anyhow::Result;
use tracing::info;
use url::Url;

use crate::ipc::bridge::IpcMessage;

/// Wraps the underlying web engine (Servo, wry, or another backend)
/// and provides a uniform interface for navigation, JavaScript injection,
/// and IPC.
///
/// The current implementation is a scaffold — the actual Servo or wry
/// integration will be wired in once the engine dependency is finalized.
pub struct WebViewEngine {
    current_url: Url,
    devtools: bool,
}

impl WebViewEngine {
    /// Create a new webview engine.
    ///
    /// In the full implementation this creates the platform web engine
    /// (Servo or wry WebView) attached to the tao window.
    pub fn new(
        initial_url: &Url,
        devtools: bool,
    ) -> Result<Self> {
        info!(
            url = %initial_url,
            devtools,
            "webview engine initialized (scaffold — no web engine linked yet)"
        );
        Ok(Self {
            current_url: initial_url.clone(),
            devtools,
        })
    }

    /// Navigate the engine to a new URL.
    pub fn navigate(&mut self, url: &Url) {
        info!(url = %url, "engine: navigate");
        self.current_url = url.clone();
        // TODO: forward to actual web engine
    }

    /// Evaluate JavaScript in the engine's context.
    ///
    /// The result is not returned — use IPC for bidirectional communication.
    pub fn evaluate_js(&self, script: &str) {
        info!(len = script.len(), "engine: evaluate_js (no-op scaffold)");
        let _ = script;
        // TODO: forward to actual web engine
    }

    /// Inject the IPC bridge script into the webview.
    pub fn inject_ipc_bridge(&self) {
        let script = crate::ipc::bridge::IpcBridge::js_init_script();
        self.evaluate_js(script);
    }

    /// Handle an incoming IPC message from the JS side.
    pub fn handle_ipc_message(&self, msg: &IpcMessage) {
        info!(?msg, "engine: received IPC message");
    }

    /// Current URL the engine is displaying.
    #[must_use]
    pub fn current_url(&self) -> &Url {
        &self.current_url
    }

    /// Whether devtools are enabled.
    #[must_use]
    pub fn devtools_enabled(&self) -> bool {
        self.devtools
    }
}
