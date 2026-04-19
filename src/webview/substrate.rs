//! The full nami-core substrate pipeline, ported to namimado's navigate
//! path.
//!
//! Loads `(defstate)` / `(defeffect)` / `(defpredicate)` / `(defplan)` /
//! `(defagent)` / `(defroute)` / `(defquery)` / `(defderived)` /
//! `(defcomponent)` from `$XDG_CONFIG_HOME/namimado/extensions.lisp`
//! and `(defdom-transform)` / `(defframework-alias)` from
//! `$XDG_CONFIG_HOME/namimado/transforms.lisp` + `aliases.lisp`.
//!
//! On every navigate:
//!   1. fetch via blocking reqwest
//!   2. parse DOM + CSS (nami-core)
//!   3. framework detect + embedded state extraction
//!   4. route match → bind params into state store
//!   5. run queries named by the route's on-match list
//!   6. run effects (derived-aware) → may mutate state
//!   7. decide agent transform list
//!   8. expand component-flavored transforms → HTML
//!   9. expand framework aliases
//!   10. apply transforms to DOM
//!
//! The `NavigateOutcome` captures every observable — what fired, what
//! changed, the final DOM as sexp — so the chrome inspector panel can
//! render a live substrate view without re-running the pipeline.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use nami_core::agent::AgentRegistry;
use nami_core::alias::AliasRegistry;
use nami_core::component::ComponentRegistry;
use nami_core::derived::DerivedRegistry;
use nami_core::dom::Document;
use nami_core::effect::EffectRegistry;
use nami_core::plan::PlanRegistry;
use nami_core::predicate::PredicateRegistry;
use nami_core::query::QueryRegistry;
use nami_core::route::RouteRegistry;
use nami_core::store::StateStore;
use nami_core::transform::{DomTransformSpec, TransformReport};
use serde_json::Value;
use tracing::{info, warn};
use url::Url;

/// Summary of one navigate pass — enough for the inspector panel to
/// render without re-running anything.
#[derive(Debug, Clone, Default)]
pub struct SubstrateReport {
    pub frameworks: Vec<(String, f32)>,
    pub effects_fired: usize,
    pub agents_fired: usize,
    pub transforms_applied: usize,
    pub routes_matched: Option<String>,
    pub queries_dispatched: Vec<String>,
    pub state_snapshot: Vec<(String, Value)>,
    pub derived_snapshot: Vec<(String, Value)>,
    pub transform_hits: Vec<String>,
}

/// The outcome of navigating to a URL.
#[derive(Debug, Clone)]
pub struct NavigateOutcome {
    pub final_url: Url,
    pub fetched_bytes: usize,
    pub title: Option<String>,
    pub text_render: String,
    /// Post-transform DOM as an S-expression — the page absorbed into
    /// Lisp space. Depth-capped at 8 by default so deeply-nested app
    /// shells don't explode the payload.
    pub dom_sexp: String,
    pub report: SubstrateReport,
}

/// Loaded substrate + transforms + aliases + state store, plus a
/// blocking HTTP client. Persists across navigates so state cells
/// accumulate and effect history is real.
pub struct SubstratePipeline {
    effects: EffectRegistry,
    predicates: PredicateRegistry,
    plans: PlanRegistry,
    agents: AgentRegistry,
    routes: RouteRegistry,
    queries: QueryRegistry,
    derived: DerivedRegistry,
    components: ComponentRegistry,

    transforms: Vec<DomTransformSpec>,
    aliases: AliasRegistry,

    state_store: StateStore,
    http: reqwest::blocking::Client,
}

impl SubstratePipeline {
    pub fn load() -> Self {
        let cfg_dir = config_dir();
        let extensions = cfg_dir.as_ref().map(|d| d.join("extensions.lisp"));
        let transforms_path = cfg_dir.as_ref().map(|d| d.join("transforms.lisp"));
        let aliases_path = cfg_dir.as_ref().map(|d| d.join("aliases.lisp"));

        let ext_src = extensions
            .as_deref()
            .and_then(read_if_exists)
            .unwrap_or_default();
        let tfm_src = transforms_path
            .as_deref()
            .and_then(read_if_exists)
            .unwrap_or_default();
        let alias_src = aliases_path
            .as_deref()
            .and_then(read_if_exists)
            .unwrap_or_default();

        let states = nami_core::store::compile(&ext_src).unwrap_or_default();
        let mut effects = EffectRegistry::new();
        effects.extend(nami_core::effect::compile(&ext_src).unwrap_or_default());
        let mut predicates = PredicateRegistry::new();
        predicates.extend(nami_core::predicate::compile(&ext_src).unwrap_or_default());
        let mut plans = PlanRegistry::new();
        plans.extend(nami_core::plan::compile(&ext_src).unwrap_or_default());
        let mut agents = AgentRegistry::new();
        agents.extend(nami_core::agent::compile(&ext_src).unwrap_or_default());
        let mut routes = RouteRegistry::new();
        routes.extend(nami_core::route::compile(&ext_src).unwrap_or_default());
        let mut queries = QueryRegistry::new();
        queries.extend(nami_core::query::compile(&ext_src).unwrap_or_default());
        let mut derived = DerivedRegistry::new();
        derived.extend(nami_core::derived::compile(&ext_src).unwrap_or_default());
        let mut components = ComponentRegistry::new();
        components.extend(nami_core::component::compile(&ext_src).unwrap_or_default());

        let transforms = nami_core::transform::compile(&tfm_src).unwrap_or_default();
        let mut aliases = AliasRegistry::new();
        aliases.extend(nami_core::alias::compile(&alias_src).unwrap_or_default());

        let state_store = StateStore::from_specs(&states);

        let http = reqwest::blocking::Client::builder()
            .user_agent("namimado/0.1 (+https://github.com/pleme-io/namimado)")
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());

        info!(
            "substrate loaded: {} state · {} effect · {} predicate · {} plan · {} agent · {} route · {} query · {} derived · {} component · {} transform · {} alias",
            states.len(),
            effects.len(),
            predicates.len(),
            plans.len(),
            agents.len(),
            routes.len(),
            queries.len(),
            derived.len(),
            components.len(),
            transforms.len(),
            aliases.len(),
        );

        Self {
            effects,
            predicates,
            plans,
            agents,
            routes,
            queries,
            derived,
            components,
            transforms,
            aliases,
            state_store,
            http,
        }
    }

    /// Live snapshot of the state store (accumulates across navigates).
    /// The HTTP/MCP surfaces expose this as `GET /state`.
    #[must_use]
    pub fn state_snapshot(&self) -> Vec<(String, Value)> {
        self.state_store.snapshot().into_iter().collect()
    }

    pub fn navigate(&mut self, url: &Url) -> Result<NavigateOutcome> {
        let body = self.fetch(url)?;
        let mut doc = Document::parse(&body);
        let detections = nami_core::framework::detect(&doc);
        let page_state = nami_core::state::extract(&doc);

        let mut report = SubstrateReport::default();
        report.frameworks = detections
            .iter()
            .map(|d| (format!("{:?}", d.framework), d.confidence))
            .collect();

        // Phase 0 — route match, bind params.
        let mut route_on_match: Vec<String> = Vec::new();
        if !self.routes.is_empty() {
            if let Some(m) = self.routes.match_url(url.as_str()) {
                for (cell, param) in &m.bindings {
                    if let Some(val) = m.params.get(param) {
                        self.state_store.set(cell, Value::String(val.clone()));
                    }
                }
                report.routes_matched = Some(m.route.clone());
                route_on_match = m.on_match;
            }
        }

        // Phase 0.5 — dispatch queries from on-match names.
        let mut remaining: Vec<String> = Vec::new();
        if !self.queries.is_empty() {
            let fetcher = BlockingFetcher { client: &self.http };
            for name in &route_on_match {
                if self.queries.get(name).is_some() {
                    match self.queries.run(name, &fetcher, &self.state_store) {
                        Ok(r) => {
                            report.queries_dispatched.push(r.query.clone());
                        }
                        Err(e) => warn!(query = %name, error = %e, "query dispatch failed"),
                    }
                } else {
                    remaining.push(name.clone());
                }
            }
        } else {
            remaining = route_on_match;
        }
        let route_on_match = remaining;

        // Phase 1 — effects (derived-aware).
        if !self.effects.is_empty() {
            let cx = nami_core::predicate::EvalContext {
                doc: &doc,
                detections: &detections,
                state: &page_state,
            };
            let (_decisions, reports) = nami_core::effect::run_page_load_with_derived(
                &self.state_store,
                &self.effects,
                &self.derived,
                &self.predicates,
                &cx,
            );
            report.effects_fired = reports.iter().filter(|r| r.ok).count();
        }

        // Phase 2 — agent decisions.
        let agent_names: Vec<String> = if self.agents.is_empty() {
            Vec::new()
        } else {
            let cx = nami_core::predicate::EvalContext {
                doc: &doc,
                detections: &detections,
                state: &page_state,
            };
            let decisions = nami_core::agent::decide(
                &self.agents,
                "page-load",
                &self.predicates,
                &self.plans,
                &cx,
            );
            report.agents_fired = decisions.iter().filter(|d| d.fired).count();
            decisions
                .into_iter()
                .filter(|d| d.fired)
                .flat_map(|d| d.transforms)
                .collect()
        };

        // Merge agent-decided + route-matched on-match names. Fallback to
        // every transform when neither fired anything.
        let mut decided_names = route_on_match;
        decided_names.extend(agent_names);
        let selected: Vec<DomTransformSpec> = if decided_names.is_empty() {
            self.transforms.clone()
        } else {
            decided_names
                .iter()
                .filter_map(|name| self.transforms.iter().find(|t| &t.name == name).cloned())
                .collect()
        };

        // Phase 3 — component expansion, alias expansion, apply.
        if !selected.is_empty() {
            let with_components = if self.components.is_empty() {
                selected
            } else {
                nami_core::transform::resolve_components(&selected, &self.components)
            };
            let fully_resolved = if self.aliases.is_empty() {
                with_components
            } else {
                self.aliases
                    .expand_transforms(&with_components, &detections)
            };
            let tfm_report: TransformReport =
                nami_core::transform::apply(&mut doc, &fully_resolved);
            report.transforms_applied = tfm_report.applied.len();
            report.transform_hits = tfm_report
                .applied
                .iter()
                .map(|h| format!("{} ({:?} on <{}>)", h.transform, h.action, h.tag))
                .collect();
        }

        // Snapshot state + derived for the inspector.
        report.state_snapshot = self
            .state_store
            .snapshot()
            .into_iter()
            .collect();
        report.derived_snapshot = match self.derived.evaluate_all(&self.state_store) {
            Ok(map) => map.into_iter().collect(),
            Err(e) => {
                warn!(error = %e, "derived evaluate_all failed");
                Vec::new()
            }
        };

        let title = doc.title();
        let text_render = visible_text(&doc);
        let dom_sexp = nami_core::lisp::dom_to_sexp_with(
            &doc,
            &nami_core::lisp::SexpOptions {
                depth_cap: Some(8),
                pretty: true,
                trim_whitespace: true,
            },
        );

        Ok(NavigateOutcome {
            final_url: url.clone(),
            fetched_bytes: body.len(),
            title,
            text_render,
            dom_sexp,
            report,
        })
    }

    fn fetch(&self, url: &Url) -> Result<String> {
        let scheme = url.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(anyhow!("unsupported scheme: {scheme}"));
        }
        let resp = self
            .http
            .get(url.as_str())
            .send()
            .with_context(|| format!("fetch {url}"))?;
        let body = resp
            .text()
            .with_context(|| format!("read body from {url}"))?;
        Ok(body)
    }
}

fn config_dir() -> Option<PathBuf> {
    // Honour XDG_CONFIG_HOME on every platform (matches aranami +
    // nami-core); fall back to ~/.config so macOS users get the same
    // ~/.config/namimado/ path they expect from other pleme-io tools.
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("namimado"));
        }
    }
    dirs::home_dir().map(|h| h.join(".config").join("namimado"))
}

fn read_if_exists(path: &Path) -> Option<String> {
    if path.exists() {
        std::fs::read_to_string(path).ok()
    } else {
        None
    }
}

/// Visible-text extraction — excludes `<script>`, `<style>`, `<noscript>`
/// subtrees so the page body shown in the GUI / inspector UI reads like
/// the actual content, not CSS and JS source. `Document::text_content()`
/// concatenates everything indiscriminately; this variant walks the tree
/// skipping non-content elements.
fn visible_text(doc: &nami_core::dom::Document) -> String {
    fn walk(node: &nami_core::dom::Node, out: &mut String) {
        if let Some(el) = node.as_element() {
            let tag = el.tag.to_ascii_lowercase();
            if matches!(tag.as_str(), "script" | "style" | "noscript" | "template") {
                return;
            }
        }
        if let Some(t) = node.as_text() {
            out.push_str(t);
        }
        for c in &node.children {
            walk(c, out);
        }
    }
    let mut out = String::new();
    walk(&doc.root, &mut out);
    out
}

/// Blocking reqwest adapter for nami-core's `Fetcher` trait.
struct BlockingFetcher<'a> {
    client: &'a reqwest::blocking::Client,
}

impl nami_core::query::Fetcher for BlockingFetcher<'_> {
    fn fetch(
        &self,
        url: &str,
        method: &str,
        body: Option<&str>,
        headers: &[nami_core::query::HeaderPair],
    ) -> Result<String, String> {
        let mut req = match method.to_ascii_uppercase().as_str() {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            "DELETE" => self.client.delete(url),
            other => return Err(format!("unsupported method: {other}")),
        };
        for (k, v) in headers {
            req = req.header(k, v);
        }
        if let Some(b) = body {
            req = req.body(b.to_owned());
        }
        let resp = req.send().map_err(|e| format!("send: {e}"))?;
        resp.text().map_err(|e| format!("read body: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_loads_without_panic() {
        // Any user config is tolerated — missing files = empty registries,
        // malformed ones warn + fall back.
        let _ = SubstratePipeline::load();
    }

    #[test]
    fn report_default_is_empty() {
        let r = SubstrateReport::default();
        assert_eq!(r.effects_fired, 0);
        assert_eq!(r.agents_fired, 0);
        assert_eq!(r.transforms_applied, 0);
        assert!(r.state_snapshot.is_empty());
    }
}
