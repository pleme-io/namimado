//! Webview engine abstraction.
//!
//! When the `browser-core` feature is on, this drives every navigate
//! through the full nami-core substrate pipeline: fetch → parse →
//! framework detect → state/derived binding → effects → agent decisions
//! → transforms + component splicing. The rendered text output is
//! stored for the chrome to surface; the structured `SubstrateReport`
//! lets the inspector panel show exactly what fired per page.
//!
//! Without `browser-core` the engine is a logging scaffold — useful for
//! smoke-testing the window + chrome layers.

use anyhow::Result;
use tracing::info;
use url::Url;

use crate::ipc::bridge::IpcMessage;

#[cfg(feature = "browser-core")]
use super::substrate;

#[cfg(feature = "browser-core")]
pub use substrate::NavigateOutcome;

pub struct WebViewEngine {
    current_url: Url,
    devtools: bool,

    #[cfg(feature = "browser-core")]
    pipeline: substrate::SubstratePipeline,

    #[cfg(feature = "browser-core")]
    last_outcome: Option<NavigateOutcome>,
}

impl WebViewEngine {
    pub fn new(initial_url: &Url, devtools: bool) -> Result<Self> {
        info!(
            url = %initial_url,
            devtools,
            feature_browser_core = cfg!(feature = "browser-core"),
            "webview engine initialized"
        );

        #[cfg(feature = "browser-core")]
        let pipeline = substrate::SubstratePipeline::load();

        Ok(Self {
            current_url: initial_url.clone(),
            devtools,

            #[cfg(feature = "browser-core")]
            pipeline,

            #[cfg(feature = "browser-core")]
            last_outcome: None,
        })
    }

    /// Navigate the engine to a new URL.
    ///
    /// With `browser-core`, this actually fetches + renders + runs the
    /// full Lisp substrate pipeline and stores the outcome. Without it,
    /// it's a log-only stub.
    pub fn navigate(&mut self, url: &Url) {
        info!(url = %url, "engine: navigate");
        self.current_url = url.clone();

        #[cfg(feature = "browser-core")]
        {
            match self.pipeline.navigate(url) {
                Ok(outcome) => {
                    info!(
                        bytes = outcome.fetched_bytes,
                        transforms = outcome.report.transforms_applied,
                        effects = outcome.report.effects_fired,
                        agents = outcome.report.agents_fired,
                        frameworks = outcome.report.frameworks.len(),
                        "page loaded via nami-core substrate pipeline"
                    );
                    self.last_outcome = Some(outcome);
                }
                Err(e) => {
                    tracing::warn!(url = %url, error = %e, "navigate failed");
                }
            }
        }
    }

    pub fn evaluate_js(&self, script: &str) {
        info!(len = script.len(), "engine: evaluate_js (no-op — no JS engine yet)");
        let _ = script;
    }

    pub fn inject_ipc_bridge(&self) {
        let script = crate::ipc::bridge::IpcBridge::js_init_script();
        self.evaluate_js(script);
    }

    pub fn handle_ipc_message(&self, msg: &IpcMessage) {
        info!(?msg, "engine: received IPC message");
    }

    #[must_use]
    pub fn current_url(&self) -> &Url {
        &self.current_url
    }

    #[must_use]
    pub fn devtools_enabled(&self) -> bool {
        self.devtools
    }

    /// Snapshot of the most recent navigate, when browser-core is on.
    /// The chrome inspector panel reads this to show substrate activity.
    #[cfg(feature = "browser-core")]
    #[must_use]
    pub fn last_outcome(&self) -> Option<&NavigateOutcome> {
        self.last_outcome.as_ref()
    }
}
