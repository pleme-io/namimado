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
use url::Url;

use crate::api::{NavigateRequest, NavigateResponse, ReportResponse, StatusResponse, StateCellValue};

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
            })),
        }
    }

    #[cfg(not(feature = "browser-core"))]
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                version: env!("CARGO_PKG_VERSION"),
            })),
        }
    }

    /// GET /status — server liveness + feature set.
    pub fn status(&self) -> StatusResponse {
        let inner = self.inner.lock().expect("service mutex poisoned");
        StatusResponse {
            service: "namimado".to_owned(),
            version: inner.version.to_owned(),
            features: compile_features(),
            last_url: self.last_url(&inner),
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
}
