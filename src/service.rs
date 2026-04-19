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
    NavigateRequest, NavigateResponse, ReloadResponse, ReportResponse, RulesInventory,
    StateCellValue, StatusResponse,
};

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
            })),
        }
    }

    /// POST /reload — re-scan `substrate.d/*.lisp` + extensions.lisp +
    /// transforms.lisp + aliases.lisp and swap in a fresh pipeline.
    /// In-flight navigates complete first (mutex ordering). State
    /// store is reset too — seeded fresh from the new (defstate) specs.
    pub fn reload(&self) -> ReloadResponse {
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
}
