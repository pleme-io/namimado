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
use nami_core::blocker::{BlockerRegistry, BlockerSpec};
use nami_core::command::{BindRegistry, BindSpec, CommandRegistry, CommandSpec, SequenceMatch};
use nami_core::extension::{ExtensionRegistry, ExtensionSpec};
use nami_core::omnibox::{OmniboxRegistry, OmniboxSpec};
use nami_core::reader::{ReaderOutput, ReaderRegistry, ReaderSpec};
use nami_core::normalize::NormalizeRegistry;
use nami_core::plan::PlanRegistry;
use nami_core::storage::kv::{Store, StorageRegistry, StorageSpec};
use std::collections::HashMap;
use nami_core::wasm::{WasmAgentContext, WasmHost};
use nami_core::wasm_agent::{WasmAgentRegistry, WasmAgentSpec};
use std::sync::Arc;
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
    /// Inline `<l-eval>` / `<script type=application/tatara-lisp>`
    /// macros processed this pass.
    pub inline_lisp_evaluated: usize,
    pub inline_lisp_failed: usize,
    /// Canonical-form rewrites applied by `(defnormalize …)` rules.
    pub normalize_applied: usize,
    pub normalize_hits: Vec<String>,
    /// `(defwasm-agent …)` scrapers that fired. Each entry is
    /// `"name → N bytes (fuel=F ms=M)"`.
    pub wasm_agents_fired: usize,
    pub wasm_agent_hits: Vec<String>,
    /// Elements stripped by `(defblocker …)` rules this navigate.
    pub blocker_applied: usize,
    pub blocker_hits: Vec<String>,
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

/// Result of dispatching a typed key sequence against the bind
/// registry. `Run` fires the command; `Prefix` means "wait for more
/// keys"; `Miss` cancels the sequence. Mirrors the substrate
/// `SequenceMatch` enum with the resolved command attached.
#[derive(Debug, Clone)]
pub enum KeyDispatch {
    Run {
        bind: BindSpec,
        command: Option<CommandSpec>,
    },
    Prefix,
    Miss,
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
    normalize_rules: NormalizeRegistry,
    blockers: BlockerRegistry,
    extensions: Arc<std::sync::Mutex<ExtensionRegistry>>,
    readers: ReaderRegistry,
    commands: CommandRegistry,
    binds: BindRegistry,
    omniboxes: OmniboxRegistry,
    wasm_agents: WasmAgentRegistry,
    wasm_host: Option<WasmHost>,
    storage_registry: StorageRegistry,
    stores: HashMap<String, Store>,

    transforms: Vec<DomTransformSpec>,
    aliases: AliasRegistry,

    state_store: StateStore,
    http: reqwest::blocking::Client,

    // Name indexes for the /rules inventory surface. Populated at
    // load time; registries themselves don't expose a uniform iter API.
    effect_names: Vec<String>,
    predicate_names: Vec<String>,
    plan_names: Vec<String>,
    agent_names: Vec<String>,
    route_names: Vec<String>,
    query_names: Vec<String>,
    derived_names: Vec<String>,
    component_names: Vec<String>,
    normalize_names: Vec<String>,
    alias_names: Vec<String>,
    wasm_agent_names: Vec<String>,
    blocker_names: Vec<String>,
    storage_names: Vec<String>,
    extension_names: Vec<String>,
    reader_names: Vec<String>,
    command_names: Vec<String>,
    bind_chords: Vec<String>,
    omnibox_names: Vec<String>,
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

        // Pick up any `.lisp` file dropped into `substrate.d/` so users
        // can install rule packs without editing extensions.lisp.
        // Files are read in sorted order for determinism; errors on any
        // single file degrade to a warning + skip.
        let drop_in_src = cfg_dir
            .as_ref()
            .map(|d| d.join("substrate.d"))
            .map(load_drop_in_dir)
            .unwrap_or_default();
        let ext_src = if drop_in_src.is_empty() {
            ext_src
        } else {
            format!("{ext_src}\n{drop_in_src}")
        };

        let states = nami_core::store::compile(&ext_src).unwrap_or_default();

        let effect_specs = nami_core::effect::compile(&ext_src).unwrap_or_default();
        let effect_names: Vec<String> = effect_specs.iter().map(|s| s.name.clone()).collect();
        let mut effects = EffectRegistry::new();
        effects.extend(effect_specs);

        let pred_specs = nami_core::predicate::compile(&ext_src).unwrap_or_default();
        let predicate_names: Vec<String> = pred_specs.iter().map(|s| s.name.clone()).collect();
        let mut predicates = PredicateRegistry::new();
        predicates.extend(pred_specs);

        let plan_specs = nami_core::plan::compile(&ext_src).unwrap_or_default();
        let plan_names: Vec<String> = plan_specs.iter().map(|s| s.name.clone()).collect();
        let mut plans = PlanRegistry::new();
        plans.extend(plan_specs);

        let agent_specs = nami_core::agent::compile(&ext_src).unwrap_or_default();
        let agent_names: Vec<String> = agent_specs.iter().map(|s| s.name.clone()).collect();
        let mut agents = AgentRegistry::new();
        agents.extend(agent_specs);

        let route_specs = nami_core::route::compile(&ext_src).unwrap_or_default();
        let route_names: Vec<String> = route_specs
            .iter()
            .map(|s| s.name.clone().unwrap_or_else(|| s.pattern.clone()))
            .collect();
        let mut routes = RouteRegistry::new();
        routes.extend(route_specs);

        let query_specs = nami_core::query::compile(&ext_src).unwrap_or_default();
        let query_names: Vec<String> = query_specs.iter().map(|s| s.name.clone()).collect();
        let mut queries = QueryRegistry::new();
        queries.extend(query_specs);

        let derived_specs = nami_core::derived::compile(&ext_src).unwrap_or_default();
        let derived_names: Vec<String> = derived_specs.iter().map(|s| s.name.clone()).collect();
        let mut derived = DerivedRegistry::new();
        derived.extend(derived_specs);

        let component_specs = nami_core::component::compile(&ext_src).unwrap_or_default();
        let component_names: Vec<String> =
            component_specs.iter().map(|s| s.name.clone()).collect();
        let mut components = ComponentRegistry::new();
        components.extend(component_specs);

        let normalize_specs = nami_core::normalize::compile(&ext_src).unwrap_or_default();
        let normalize_names: Vec<String> =
            normalize_specs.iter().map(|s| s.name.clone()).collect();
        let mut normalize_rules = NormalizeRegistry::new();
        normalize_rules.extend(normalize_specs);

        let storage_specs: Vec<StorageSpec> =
            nami_core::storage::kv::compile(&ext_src).unwrap_or_default();
        let storage_names: Vec<String> =
            storage_specs.iter().map(|s| s.name.clone()).collect();
        // Resolve relative paths against the runtime data dir; absolute
        // paths pass through. A missing path keeps the store volatile.
        let data_root = data_dir();
        let mut storage_registry = StorageRegistry::new();
        let mut stores: HashMap<String, Store> = HashMap::new();
        for mut spec in storage_specs {
            if let Some(path) = spec.path.take() {
                let resolved = if path.is_absolute() {
                    path
                } else {
                    data_root.join(path)
                };
                spec.path = Some(resolved);
            }
            let store = Store::from_spec(&spec);
            stores.insert(spec.name.clone(), store);
            storage_registry.insert(spec);
        }

        let blocker_specs: Vec<BlockerSpec> =
            nami_core::blocker::compile(&ext_src).unwrap_or_default();
        let blocker_names: Vec<String> = blocker_specs.iter().map(|s| s.name.clone()).collect();
        let mut blockers = BlockerRegistry::new();
        blockers.extend(blocker_specs);

        let extension_specs: Vec<ExtensionSpec> =
            nami_core::extension::compile(&ext_src).unwrap_or_default();
        let extension_names: Vec<String> =
            extension_specs.iter().map(|s| s.name.clone()).collect();
        let mut extension_registry = ExtensionRegistry::new();
        extension_registry.extend(extension_specs);
        let extensions = Arc::new(std::sync::Mutex::new(extension_registry));

        // Omnibox profiles — if none declared, register the built-in
        // default so /omnibox works out of the box.
        let omnibox_specs: Vec<OmniboxSpec> =
            nami_core::omnibox::compile(&ext_src).unwrap_or_default();
        let omnibox_names: Vec<String> = if omnibox_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            omnibox_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut omniboxes = OmniboxRegistry::new();
        if omnibox_specs.is_empty() {
            omniboxes.insert(OmniboxSpec::default_profile());
        } else {
            omniboxes.extend(omnibox_specs);
        }

        // Commands + bindings. Compile from the full ext_src so users
        // can drop keyboard packs into substrate.d/.
        let command_specs: Vec<CommandSpec> =
            nami_core::command::compile_commands(&ext_src).unwrap_or_default();
        let command_names: Vec<String> =
            command_specs.iter().map(|s| s.name.clone()).collect();
        let mut commands = CommandRegistry::new();
        commands.extend(command_specs);

        let bind_specs: Vec<BindSpec> =
            nami_core::command::compile_binds(&ext_src).unwrap_or_default();
        let mut binds = BindRegistry::new();
        binds.extend(bind_specs);
        let bind_chords = binds.chords();

        // Reader profiles — if none declared, register the built-in
        // default so /reader works out of the box on any page.
        let reader_specs: Vec<ReaderSpec> =
            nami_core::reader::compile(&ext_src).unwrap_or_default();
        let reader_names: Vec<String> = if reader_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            reader_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut readers = ReaderRegistry::new();
        if reader_specs.is_empty() {
            readers.insert(ReaderSpec::default_profile());
        } else {
            readers.extend(reader_specs);
        }

        let wasm_agent_specs: Vec<WasmAgentSpec> =
            nami_core::wasm_agent::compile(&ext_src).unwrap_or_default();
        let wasm_agent_names: Vec<String> =
            wasm_agent_specs.iter().map(|s| s.name.clone()).collect();
        let mut wasm_agents = WasmAgentRegistry::new();
        wasm_agents.extend(wasm_agent_specs);

        // Spin up one WasmHost we'll reuse across navigates — the JIT
        // cost is only paid once per process.
        let wasm_host = if wasm_agents.is_empty() {
            None
        } else {
            match WasmHost::new() {
                Ok(h) => Some(h),
                Err(e) => {
                    warn!("WasmHost init failed, wasm agents disabled: {e}");
                    None
                }
            }
        };

        let transforms = nami_core::transform::compile(&tfm_src).unwrap_or_default();

        let alias_specs = nami_core::alias::compile(&alias_src).unwrap_or_default();
        let alias_names: Vec<String> = alias_specs.iter().map(|s| s.name.clone()).collect();
        let mut aliases = AliasRegistry::new();
        aliases.extend(alias_specs);

        let state_store = StateStore::from_specs(&states);

        let http = reqwest::blocking::Client::builder()
            .user_agent("namimado/0.1 (+https://github.com/pleme-io/namimado)")
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());

        info!(
            "substrate loaded: {} state · {} effect · {} predicate · {} plan · {} agent · {} route · {} query · {} derived · {} component · {} transform · {} alias · {} normalize · {} wasm-agent · {} blocker · {} storage · {} extension · {} reader · {} command · {} bind · {} omnibox",
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
            normalize_rules.len(),
            wasm_agents.len(),
            blockers.len(),
            storage_registry.len(),
            extensions.lock().map(|r| r.len()).unwrap_or(0),
            readers.len(),
            commands.len(),
            binds.len(),
            // Omnibox count is cheap to surface too.
            omniboxes.len(),
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
            normalize_rules,
            blockers,
            extensions,
            readers,
            commands,
            binds,
            omniboxes,
            wasm_agents,
            wasm_host,
            storage_registry,
            stores,
            transforms,
            aliases,
            state_store,
            http,
            effect_names,
            predicate_names,
            plan_names,
            agent_names,
            route_names,
            query_names,
            derived_names,
            component_names,
            normalize_names,
            alias_names,
            wasm_agent_names,
            blocker_names,
            storage_names,
            extension_names,
            reader_names,
            command_names,
            bind_chords,
            omnibox_names,
        }
    }

    /// Rank autocomplete suggestions against `query`. Uses the named
    /// profile or `default`. Caller passes history + bookmarks from
    /// NamimadoService so we stay unaware of app-level types.
    #[must_use]
    pub fn omnibox_rank(
        &self,
        query: &str,
        profile: Option<&str>,
        history: &[nami_core::omnibox::HistoryItem],
        bookmarks: &[nami_core::omnibox::BookmarkItem],
        tabs: &[nami_core::omnibox::TabItem],
        extensions: &[nami_core::omnibox::ExtensionItem],
    ) -> Vec<nami_core::omnibox::Suggestion> {
        // Pull commands straight from our CommandRegistry + bindings.
        let commands: Vec<nami_core::omnibox::CommandItem> = self
            .commands
            .specs()
            .iter()
            .map(|c| nami_core::omnibox::CommandItem {
                name: c.name.clone(),
                description: c.description.clone(),
                bound_keys: self
                    .binds
                    .specs()
                    .iter()
                    .filter(|b| b.command == c.name)
                    .map(|b| b.canonical_key())
                    .collect(),
            })
            .collect();

        let spec = profile
            .and_then(|n| self.omniboxes.get(n))
            .or_else(|| self.omniboxes.specs().first())
            .cloned()
            .unwrap_or_else(OmniboxSpec::default_profile);

        nami_core::omnibox::rank(
            query,
            &spec,
            nami_core::omnibox::OmniboxInput {
                history,
                bookmarks,
                commands: &commands,
                tabs,
                extensions,
            },
        )
    }

    /// Every defined omnibox profile's name.
    #[must_use]
    pub fn omnibox_names(&self) -> &[String] {
        &self.omnibox_names
    }

    /// Dispatch a typed-so-far key sequence in `mode` against the
    /// bind registry. Returns whichever of Complete/Prefix/Miss fits;
    /// the caller advances the sequence state or invokes the command.
    #[must_use]
    pub fn dispatch_key(&self, typed: &str, mode: &str) -> KeyDispatch {
        match self.binds.match_sequence(typed, mode) {
            SequenceMatch::Complete(bind) => {
                let command = self
                    .commands
                    .get(&bind.command)
                    .cloned();
                KeyDispatch::Run {
                    bind: bind.clone(),
                    command,
                }
            }
            SequenceMatch::Prefix => KeyDispatch::Prefix,
            SequenceMatch::Miss => KeyDispatch::Miss,
        }
    }

    /// Full command + binding inventory for the inspector / MCP.
    /// Returns (command_names, chord_strings).
    #[must_use]
    pub fn keybindings_summary(&self) -> (Vec<String>, Vec<String>) {
        (self.command_names.clone(), self.bind_chords.clone())
    }

    /// Every command + every chord that invokes it, joined. Powers
    /// GET /commands and the MCP `commands_list` tool.
    #[must_use]
    pub fn commands_inventory(&self) -> Vec<crate::api::CommandInfo> {
        self.commands
            .specs()
            .iter()
            .map(|c| {
                let bound_keys: Vec<String> = self
                    .binds
                    .specs()
                    .iter()
                    .filter(|b| b.command == c.name)
                    .map(|b| b.canonical_key())
                    .collect();
                crate::api::CommandInfo {
                    name: c.name.clone(),
                    description: c.description.clone(),
                    action: c.action.clone(),
                    body: c.body.clone(),
                    default_key: c.default_key.clone(),
                    bound_keys,
                }
            })
            .collect()
    }

    /// Apply a named reader profile to the last-parsed document
    /// (supplied by the caller — SubstratePipeline doesn't retain
    /// parsed DOMs). When `name` is None, uses the first registered
    /// profile. Returns None when no profile matches.
    #[must_use]
    pub fn apply_reader(&self, doc: &Document, name: Option<&str>, host: &str) -> Option<ReaderOutput> {
        let spec = match name {
            Some(n) => self.readers.specs().iter().find(|s| s.name == n),
            None => self.readers.resolve(host).or_else(|| self.readers.specs().first()),
        }?;
        Some(nami_core::reader::apply_reader(doc, spec))
    }

    /// Installed extensions — summary. Returns (name, version,
    /// enabled, host_permission_count, rule_count) tuples.
    #[must_use]
    pub fn extension_summary(&self) -> Vec<(String, String, bool, usize, usize)> {
        let Ok(reg) = self.extensions.lock() else {
            return Vec::new();
        };
        reg.specs()
            .iter()
            .map(|s| (
                s.name.clone(),
                s.version.clone(),
                s.enabled,
                s.host_permissions.len(),
                s.rules.len(),
            ))
            .collect()
    }

    /// Full ExtensionSpec lookup by name.
    #[must_use]
    pub fn extension_get(&self, name: &str) -> Option<ExtensionSpec> {
        self.extensions.lock().ok()?.get(name).cloned()
    }

    /// Toggle enable/disable at runtime. Returns true if the extension
    /// exists and was toggled.
    pub fn extension_set_enabled(&self, name: &str, enabled: bool) -> bool {
        let Ok(mut reg) = self.extensions.lock() else {
            return false;
        };
        reg.set_enabled(name, enabled)
    }

    /// Install a new extension (or replace by name). Returns the new
    /// content hash of the registry after insertion.
    pub fn extension_install(&self, spec: ExtensionSpec) -> Option<String> {
        let mut reg = self.extensions.lock().ok()?;
        reg.insert(spec);
        Some(reg.content_hash())
    }

    /// Remove an extension by name. Returns true if removed.
    pub fn extension_remove(&self, name: &str) -> bool {
        let Ok(mut reg) = self.extensions.lock() else {
            return false;
        };
        reg.remove(name)
    }

    /// Content-addressable hash of the installed extension set.
    #[must_use]
    pub fn extensions_content_hash(&self) -> String {
        self.extensions
            .lock()
            .map(|r| r.content_hash())
            .unwrap_or_default()
    }

    /// Get a `Store` handle by name. Returns `None` when the name
    /// doesn't match any `(defstorage)` declaration in the loaded
    /// substrate. Cheap to call — the underlying map is cloned.
    #[must_use]
    pub fn get_store(&self, name: &str) -> Option<Store> {
        self.stores.get(name).cloned()
    }

    /// All configured store names + their entry counts. Powers the
    /// MCP `storage_list_stores` tool and GET /storage.
    #[must_use]
    pub fn storage_summary(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = self
            .stores
            .iter()
            .map(|(name, store)| (name.clone(), store.len()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Live snapshot of the state store (accumulates across navigates).
    /// The HTTP/MCP surfaces expose this as `GET /state`.
    #[must_use]
    pub fn state_snapshot(&self) -> Vec<(String, Value)> {
        self.state_store.snapshot().into_iter().collect()
    }

    // Helper: rules inventory requires knowing the storage names.
    // See below.
    /// Inventory of every DSL form currently loaded, by name. Powers
    /// the `/rules` endpoint and MCP `get_rules` tool.
    #[must_use]
    pub fn rules_inventory(&self) -> crate::api::RulesInventory {
        crate::api::RulesInventory {
            states: self.state_store.names(),
            effects: self.effect_names.clone(),
            predicates: self.predicate_names.clone(),
            plans: self.plan_names.clone(),
            agents: self.agent_names.clone(),
            routes: self.route_names.clone(),
            queries: self.query_names.clone(),
            derived: self.derived_names.clone(),
            components: self.component_names.clone(),
            normalize_rules: self.normalize_names.clone(),
            transforms: self.transforms.iter().map(|s| s.name.clone()).collect(),
            aliases: self.alias_names.clone(),
            wasm_agents: self.wasm_agent_names.clone(),
            blockers: self.blocker_names.clone(),
            storages: self.storage_names.clone(),
            extensions: self.extension_names.clone(),
            readers: self.reader_names.clone(),
            commands: self.command_names.clone(),
            binds: self.bind_chords.clone(),
            omniboxes: self.omnibox_names.clone(),
        }
    }

    pub fn navigate(&mut self, url: &Url) -> Result<NavigateOutcome> {
        let body = self.fetch(url)?;
        let mut doc = Document::parse(&body);

        // Phase −1 — expand inline `<l-eval>` / tatara-lisp script macros
        // BEFORE framework detection so any DOM they emit is visible to
        // downstream passes.
        let evaluator = nami_core::eval::NamiEvaluator::new();
        let inline_report = nami_core::inline_lisp::expand(&mut doc, &evaluator);

        let detections = nami_core::framework::detect(&doc);
        let page_state = nami_core::state::extract(&doc);

        // Phase 0.25 — canonicalize detected framework idioms into the
        // shared n-* vocabulary so downstream transforms, scrapes, and
        // agents author against one shape.
        let norm_report =
            nami_core::normalize::apply(&mut doc, &self.normalize_rules, &detections);

        let mut report = SubstrateReport::default();
        report.inline_lisp_evaluated = inline_report.evaluated;
        report.inline_lisp_failed = inline_report.failed;
        report.normalize_applied = norm_report.applied();
        report.normalize_hits = norm_report
            .hits
            .iter()
            .map(|h| format!("{} : {} → {}", h.rule, h.from_tag, h.to_tag))
            .collect();

        // Phase 0.3 — content blocking. Runs after canonicalization
        // so rules can target the canonical n-* shape and fire
        // uniformly across frameworks.
        let block_report = nami_core::blocker::apply(&mut doc, &self.blockers);
        report.blocker_applied = block_report.applied();
        report.blocker_hits = block_report
            .hits
            .iter()
            .map(|h| format!("{} : {} <{}>", h.rule, h.selector, h.tag))
            .collect();

        // Phase 0.4 — dispatch (defwasm-agent) scrapers. Each runs
        // against a read-only snapshot of the current doc. Output
        // bytes land in the report; failures log + continue.
        if !self.wasm_agents.is_empty() {
            if let Some(host) = &self.wasm_host {
                let snapshot = Arc::new(doc.clone());
                let cx = WasmAgentContext::with_snapshot(snapshot);
                let wasm_dir = wasm_agent_dir();
                let reports = nami_core::wasm_agent::run(
                    &self.wasm_agents,
                    "page-load",
                    host,
                    &cx,
                    |path| resolve_wasm_path(&wasm_dir, path),
                );
                report.wasm_agents_fired = reports.iter().filter(|r| r.ok()).count();
                report.wasm_agent_hits = reports
                    .iter()
                    .map(|r| match &r.outcome {
                        Ok(out) => format!(
                            "{} → {} bytes (fuel={} ms={})",
                            r.name,
                            out.len(),
                            r.fuel_used,
                            r.duration_ms
                        ),
                        Err(e) => format!("{} FAILED: {}", r.name, e),
                    })
                    .collect();
            }
        }
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
        // Outbound block check — uBlock-style pre-fetch gating. A
        // match refuses the navigate with the blocking rule name so
        // agents / users can tell *why*.
        if let Some(hit) = self.blockers.block_url(url.as_str()) {
            return Err(anyhow!("blocked by defblocker rule {:?}", hit.name));
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

fn wasm_agent_dir() -> PathBuf {
    config_dir()
        .map(|d| d.join("wasm"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Runtime data dir — `$XDG_DATA_HOME/namimado/` or
/// `~/.local/share/namimado/`. Relative `(defstorage :path …)`
/// paths resolve against this. Scheme separate from `config_dir()`
/// because storage is a different lifecycle than config — one is
/// mutable runtime state, the other is authored + reloaded.
fn data_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("namimado");
        }
    }
    dirs::home_dir()
        .map(|h| h.join(".local").join("share").join("namimado"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Resolve a `:wasm` path to bytes under a strict policy:
///   · absolute paths used verbatim
///   · relative paths joined under `$XDG_CONFIG_HOME/namimado/wasm/`
///   · no traversal outside that directory when relative
fn resolve_wasm_path(wasm_dir: &Path, path: &str) -> Result<Vec<u8>, String> {
    let p = Path::new(path);
    let resolved = if p.is_absolute() {
        p.to_path_buf()
    } else {
        // Reject `..` traversal in relative paths — relative means
        // "inside the wasm agents dir."
        if path.split('/').any(|seg| seg == "..") {
            return Err(format!("path traversal rejected: {path}"));
        }
        wasm_dir.join(path)
    };
    std::fs::read(&resolved).map_err(|e| format!("read {resolved:?}: {e}"))
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

/// Concatenate every `.lisp` file in `dir` (sorted). Returns "" when
/// the directory is absent or empty. Malformed files log and skip.
fn load_drop_in_dir(dir: PathBuf) -> String {
    if !dir.is_dir() {
        return String::new();
    }
    let mut entries: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("lisp"))
            .collect(),
        Err(_) => return String::new(),
    };
    entries.sort();
    let mut out = String::new();
    for path in entries {
        match std::fs::read_to_string(&path) {
            Ok(src) => {
                info!("loading drop-in {:?}", path.file_name().unwrap_or_default());
                out.push_str(&src);
                out.push('\n');
            }
            Err(e) => warn!("failed to read {path:?}: {e}"),
        }
    }
    out
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

    #[test]
    fn drop_in_dir_concatenates_lisp_files_in_sorted_order() {
        // substrate.d/ auto-load should include every .lisp file, in
        // deterministic sorted order.
        let tmp = std::env::temp_dir().join(format!(
            "namimado-drop-in-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("b.lisp"), ";; b\n").unwrap();
        std::fs::write(tmp.join("a.lisp"), ";; a\n").unwrap();
        std::fs::write(tmp.join("not-included.txt"), "nope").unwrap();

        let out = load_drop_in_dir(tmp.clone());
        // `a` file concatenates before `b` (sorted), .txt ignored.
        assert!(out.contains(";; a"));
        assert!(out.contains(";; b"));
        assert!(!out.contains("nope"));
        let a_pos = out.find(";; a").unwrap();
        let b_pos = out.find(";; b").unwrap();
        assert!(a_pos < b_pos, "sorted order violated");

        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn drop_in_dir_absent_yields_empty_string() {
        let tmp = std::env::temp_dir().join("namimado-no-such-dir-for-drop-in");
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(load_drop_in_dir(tmp), "");
    }
}
