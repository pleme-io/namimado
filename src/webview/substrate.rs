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
use nami_core::extension::{
    ExtensionRegistry, ExtensionSpec, SignedExtension, Trustdb, VerificationError,
    VerificationStatus,
};
use nami_core::boost::{BoostRegistry, BoostSpec};
use nami_core::find::{FindRegistry, FindSpec};
use nami_core::gesture::{GestureRegistry, GestureSpec};
use nami_core::i18n::{MessageRegistry, MessageSpec};
use nami_core::js_runtime::{
    EvalContext, EvalError, ExecutionResult, JsRuntime, JsRuntimeRegistry, JsRuntimeSpec,
    MicroEval,
};
use nami_core::omnibox::{OmniboxRegistry, OmniboxSpec};
use nami_core::pip::{PipRegistry, PipSpec};
use nami_core::session::{SessionSpec, SessionStore, TabRecord};
use nami_core::annotate::{AnnotateRegistry, AnnotateSpec};
use nami_core::auth_saver::{AuthSaverRegistry, AuthSaverSpec};
use nami_core::autofill::{AutofillRegistry, AutofillSpec};
use nami_core::chat::{ChatRegistry, ChatSpec};
use nami_core::llm::{
    EchoProvider, LlmProvider, LlmProviderRegistry, LlmProviderSpec,
};
use nami_core::llm_completion::{LlmCompletionRegistry, LlmCompletionSpec};
use nami_core::passkey::{PasskeyRegistry, PasskeySpec};
use nami_core::passwords::{PasswordsRegistry, PasswordsSpec};
use nami_core::secure_note::{SecureNoteRegistry, SecureNoteSpec};
use nami_core::summarize::{SummarizeRegistry, SummarizeSpec};
use nami_core::bridge::{BridgeRegistry, BridgeSpec};
use nami_core::crdt_room::{CrdtRoomRegistry, CrdtRoomSpec};
use nami_core::multiplayer_cursor::{MultiplayerCursorRegistry, MultiplayerCursorSpec};
use nami_core::presence::{PresenceRegistry, PresenceSpec};
use nami_core::service_worker::{ServiceWorkerRegistry, ServiceWorkerSpec};
use nami_core::sync_channel::{SyncRegistry, SyncSpec};
use nami_core::tab_group::{TabGroupRegistry, TabGroupSpec};
use nami_core::tab_hibernate::{TabHibernateRegistry, TabHibernateSpec};
use nami_core::tab_preview::{TabPreviewRegistry, TabPreviewSpec};
use nami_core::search_engine::{SearchEngineRegistry, SearchEngineSpec};
use nami_core::search_bang::{SearchBangRegistry, SearchBangSpec};
use nami_core::identity::{IdentityRegistry, IdentitySpec};
use nami_core::totp::{TotpRegistry, TotpSpec};
use nami_core::fingerprint_randomize::{FingerprintRandomizeRegistry, FingerprintRandomizeSpec};
use nami_core::cookie_jar::{CookieJarRegistry, CookieJarSpec};
use nami_core::webgpu_policy::{WebgpuPolicyRegistry, WebgpuPolicySpec};
use nami_core::cast::{CastRegistry, CastSpec};
use nami_core::console_rule::{ConsoleRuleRegistry, ConsoleRuleSpec};
use nami_core::high_contrast::{HighContrastRegistry, HighContrastSpec};
use nami_core::inspector::{InspectorRegistry, InspectorSpec};
use nami_core::profiler::{ProfilerRegistry, ProfilerSpec};
use nami_core::reader_aloud::{ReaderAloudRegistry, ReaderAloudSpec};
use nami_core::simplify::{SimplifyRegistry, SimplifySpec};
use nami_core::media_session::{MediaSessionRegistry, MediaSessionSpec};
use nami_core::subtitle::{SubtitleRegistry, SubtitleSpec};
use nami_core::dns::{DnsRegistry, DnsSpec};
use nami_core::download::{DownloadRegistry, DownloadSpec};
use nami_core::offline::{OfflineRegistry, OfflineSpec};
use nami_core::pull_refresh::{PullRefreshRegistry, PullRefreshSpec};
use nami_core::share::{ShareRegistry, ShareTargetSpec};
use nami_core::feed::{FeedRegistry, FeedSpec};
use nami_core::outline::{OutlineRegistry, OutlineSpec};
use nami_core::redirect::{RedirectRegistry, RedirectSpec};
use nami_core::routing::{RouteVia, RoutingRegistry, RoutingSpec};
use nami_core::script_policy::{ScriptPolicyRegistry, ScriptPolicySpec};
use nami_core::url_clean::{UrlCleanRegistry, UrlCleanSpec};
use nami_core::sidebar::{SidebarRegistry, SidebarSpec};
use nami_core::snapshot::{SnapshotRegistry, SnapshotSpec};
use nami_core::space::{SpaceRegistry, SpaceSpec, SpaceState};
use nami_core::split::{SplitRegistry, SplitSpec};
use nami_core::spoof::{SpoofRegistry, SpoofSpec};
use nami_core::zoom::{ZoomRegistry, ZoomSpec};
use nami_core::reader::{ReaderOutput, ReaderRegistry, ReaderSpec};
use nami_core::security_policy::{
    PolicyHeaders, SecurityPolicyRegistry, SecurityPolicySpec,
};
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
    /// Trusted signing keys for (defextension) bundles. Loaded from
    /// ~/.config/namimado/trustdb.txt (one base64 pubkey per line).
    trustdb: Arc<std::sync::Mutex<Trustdb>>,
    readers: ReaderRegistry,
    commands: CommandRegistry,
    binds: BindRegistry,
    omniboxes: OmniboxRegistry,
    messages: MessageRegistry,
    security_policies: SecurityPolicyRegistry,
    finds: FindRegistry,
    zooms: ZoomRegistry,
    snapshots: SnapshotRegistry,
    pips: PipRegistry,
    gestures: GestureRegistry,
    boosts: BoostRegistry,
    js_runtimes: JsRuntimeRegistry,
    /// Active runtime implementation. MicroEval today; real engines
    /// drop in behind feature flags via this same trait object.
    js_engine: Arc<dyn JsRuntime>,
    spaces: SpaceRegistry,
    space_state: Arc<std::sync::Mutex<SpaceState>>,
    sidebars: SidebarRegistry,
    splits: SplitRegistry,
    spoofs: SpoofRegistry,
    dnses: DnsRegistry,
    routings: RoutingRegistry,
    outlines: OutlineRegistry,
    annotates: AnnotateRegistry,
    feeds: FeedRegistry,
    redirects: RedirectRegistry,
    url_cleans: UrlCleanRegistry,
    script_policies: ScriptPolicyRegistry,
    bridges: BridgeRegistry,
    shares: ShareRegistry,
    offlines: OfflineRegistry,
    pull_refreshes: PullRefreshRegistry,
    downloads: DownloadRegistry,
    autofills: AutofillRegistry,
    passwords: PasswordsRegistry,
    auth_savers: AuthSaverRegistry,
    secure_notes: SecureNoteRegistry,
    passkeys: PasskeyRegistry,
    llm_providers: LlmProviderRegistry,
    llm_engine: Arc<dyn LlmProvider>,
    summarizes: SummarizeRegistry,
    chats: ChatRegistry,
    llm_completions: LlmCompletionRegistry,
    media_sessions: MediaSessionRegistry,
    casts: CastRegistry,
    subtitles: SubtitleRegistry,
    inspectors: InspectorRegistry,
    profilers: ProfilerRegistry,
    console_rules: ConsoleRuleRegistry,
    reader_alouds: ReaderAloudRegistry,
    high_contrasts: HighContrastRegistry,
    simplifies: SimplifyRegistry,
    presences: PresenceRegistry,
    crdt_rooms: CrdtRoomRegistry,
    multiplayer_cursors: MultiplayerCursorRegistry,
    service_workers: ServiceWorkerRegistry,
    syncs: SyncRegistry,
    tab_groups: TabGroupRegistry,
    tab_hibernates: TabHibernateRegistry,
    tab_previews: TabPreviewRegistry,
    search_engines: SearchEngineRegistry,
    search_bangs: SearchBangRegistry,
    identities: IdentityRegistry,
    totps: TotpRegistry,
    fingerprint_randomizes: FingerprintRandomizeRegistry,
    cookie_jars: CookieJarRegistry,
    webgpu_policies: WebgpuPolicyRegistry,
    session_store: Arc<std::sync::Mutex<SessionStore>>,
    session_spec: SessionSpec,
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
    i18n_namespaces: Vec<String>,
    security_policy_names: Vec<String>,
    find_names: Vec<String>,
    zoom_hosts: Vec<String>,
    snapshot_names: Vec<String>,
    pip_names: Vec<String>,
    gesture_strokes: Vec<String>,
    boost_names: Vec<String>,
    js_runtime_names: Vec<String>,
    space_names: Vec<String>,
    sidebar_names: Vec<String>,
    split_names: Vec<String>,
    spoof_names: Vec<String>,
    dns_names: Vec<String>,
    routing_names: Vec<String>,
    outline_names: Vec<String>,
    annotate_names: Vec<String>,
    feed_names: Vec<String>,
    redirect_names: Vec<String>,
    url_clean_names: Vec<String>,
    script_policy_names: Vec<String>,
    bridge_names: Vec<String>,
    share_names: Vec<String>,
    offline_names: Vec<String>,
    pull_refresh_names: Vec<String>,
    download_names: Vec<String>,
    autofill_names: Vec<String>,
    password_names: Vec<String>,
    auth_saver_names: Vec<String>,
    secure_note_names: Vec<String>,
    passkey_names: Vec<String>,
    llm_provider_names: Vec<String>,
    summarize_names: Vec<String>,
    chat_names: Vec<String>,
    llm_completion_names: Vec<String>,
    media_session_names: Vec<String>,
    cast_names: Vec<String>,
    subtitle_names: Vec<String>,
    inspector_names: Vec<String>,
    profiler_names: Vec<String>,
    console_rule_names: Vec<String>,
    reader_aloud_names: Vec<String>,
    high_contrast_names: Vec<String>,
    simplify_names: Vec<String>,
    presence_names: Vec<String>,
    crdt_room_names: Vec<String>,
    multiplayer_cursor_names: Vec<String>,
    service_worker_names: Vec<String>,
    sync_names: Vec<String>,
    tab_group_names: Vec<String>,
    tab_hibernate_names: Vec<String>,
    tab_preview_names: Vec<String>,
    search_engine_names: Vec<String>,
    search_bang_triggers: Vec<String>,
    identity_names: Vec<String>,
    totp_names: Vec<String>,
    fingerprint_randomize_names: Vec<String>,
    cookie_jar_names: Vec<String>,
    webgpu_policy_names: Vec<String>,
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

        // Trust DB — pubkeys allowed to sign extensions. One
        // base64-encoded ed25519 pubkey per line, `#`-prefixed
        // comments allowed. Silently empty if the file is absent.
        let mut trustdb = Trustdb::new();
        if let Some(path) = cfg_dir.as_ref().map(|d| d.join("trustdb.txt")) {
            if let Ok(body) = std::fs::read_to_string(&path) {
                for line in body.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    trustdb.trust(trimmed.to_owned());
                }
            }
        }
        let trustdb = Arc::new(std::sync::Mutex::new(trustdb));

        // i18n message bundles.
        let message_specs: Vec<MessageSpec> =
            nami_core::i18n::compile(&ext_src).unwrap_or_default();
        let mut i18n_ns_set: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for s in &message_specs {
            i18n_ns_set.insert(s.namespace.clone());
        }
        let i18n_namespaces: Vec<String> = i18n_ns_set.into_iter().collect();
        let mut messages = MessageRegistry::new();
        messages.extend(message_specs);

        // Tier-1 registries — compile, default-when-empty where it
        // makes sense.
        let find_specs: Vec<FindSpec> =
            nami_core::find::compile(&ext_src).unwrap_or_default();
        let find_names: Vec<String> = if find_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            find_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut finds = FindRegistry::new();
        if find_specs.is_empty() {
            finds.insert(FindSpec::default_profile());
        } else {
            finds.extend(find_specs);
        }

        let zoom_specs: Vec<ZoomSpec> =
            nami_core::zoom::compile(&ext_src).unwrap_or_default();
        let zoom_hosts: Vec<String> =
            zoom_specs.iter().map(|s| s.host.clone()).collect();
        let mut zooms = ZoomRegistry::new();
        zooms.extend(zoom_specs);

        let snap_specs: Vec<SnapshotSpec> =
            nami_core::snapshot::compile(&ext_src).unwrap_or_default();
        let snapshot_names: Vec<String> = if snap_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            snap_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut snapshots = SnapshotRegistry::new();
        if snap_specs.is_empty() {
            snapshots.insert(SnapshotSpec::default_profile());
        } else {
            snapshots.extend(snap_specs);
        }

        let pip_specs: Vec<PipSpec> =
            nami_core::pip::compile(&ext_src).unwrap_or_default();
        let pip_names: Vec<String> = if pip_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            pip_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut pips = PipRegistry::new();
        if pip_specs.is_empty() {
            pips.insert(PipSpec::default_profile());
        } else {
            pips.extend(pip_specs);
        }

        let gesture_specs: Vec<GestureSpec> =
            nami_core::gesture::compile(&ext_src).unwrap_or_default();
        let mut gestures = GestureRegistry::new();
        gestures.extend(gesture_specs);
        let gesture_strokes = gestures.strokes();

        let boost_specs: Vec<BoostSpec> =
            nami_core::boost::compile(&ext_src).unwrap_or_default();
        let boost_names: Vec<String> =
            boost_specs.iter().map(|s| s.name.clone()).collect();
        let mut boosts = BoostRegistry::new();
        boosts.extend(boost_specs);

        // Reading pack.
        let outline_specs: Vec<OutlineSpec> =
            nami_core::outline::compile(&ext_src).unwrap_or_default();
        let outline_names: Vec<String> = if outline_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            outline_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut outlines = OutlineRegistry::new();
        if outline_specs.is_empty() {
            outlines.insert(OutlineSpec::default_profile());
        } else {
            outlines.extend(outline_specs);
        }

        let annotate_specs: Vec<AnnotateSpec> =
            nami_core::annotate::compile(&ext_src).unwrap_or_default();
        let annotate_names: Vec<String> =
            annotate_specs.iter().map(|s| s.name.clone()).collect();
        let mut annotates = AnnotateRegistry::new();
        annotates.extend(annotate_specs);

        let feed_specs: Vec<FeedSpec> =
            nami_core::feed::compile(&ext_src).unwrap_or_default();
        let feed_names: Vec<String> =
            feed_specs.iter().map(|s| s.name.clone()).collect();
        let mut feeds = FeedRegistry::new();
        feeds.extend(feed_specs);

        // TOR-v2 pack.
        let redirect_specs: Vec<RedirectSpec> =
            nami_core::redirect::compile(&ext_src).unwrap_or_default();
        let redirect_names: Vec<String> =
            redirect_specs.iter().map(|s| s.name.clone()).collect();
        let mut redirects = RedirectRegistry::new();
        redirects.extend(redirect_specs);

        let url_clean_specs: Vec<UrlCleanSpec> =
            nami_core::url_clean::compile(&ext_src).unwrap_or_default();
        let url_clean_names: Vec<String> =
            url_clean_specs.iter().map(|s| s.name.clone()).collect();
        let mut url_cleans = UrlCleanRegistry::new();
        url_cleans.extend(url_clean_specs);

        let script_policy_specs: Vec<ScriptPolicySpec> =
            nami_core::script_policy::compile(&ext_src).unwrap_or_default();
        let script_policy_names: Vec<String> =
            script_policy_specs.iter().map(|s| s.name.clone()).collect();
        let mut script_policies = ScriptPolicyRegistry::new();
        script_policies.extend(script_policy_specs);

        let bridge_specs: Vec<BridgeSpec> =
            nami_core::bridge::compile(&ext_src).unwrap_or_default();
        let bridge_names: Vec<String> =
            bridge_specs.iter().map(|s| s.name.clone()).collect();
        let mut bridges = BridgeRegistry::new();
        bridges.extend(bridge_specs);

        // Accessibility-plus pack.
        let reader_aloud_specs: Vec<ReaderAloudSpec> =
            nami_core::reader_aloud::compile(&ext_src).unwrap_or_default();
        let reader_aloud_names: Vec<String> =
            reader_aloud_specs.iter().map(|s| s.name.clone()).collect();
        let mut reader_alouds = ReaderAloudRegistry::new();
        reader_alouds.extend(reader_aloud_specs);

        let high_contrast_specs: Vec<HighContrastSpec> =
            nami_core::high_contrast::compile(&ext_src).unwrap_or_default();
        let high_contrast_names: Vec<String> =
            high_contrast_specs.iter().map(|s| s.name.clone()).collect();
        let mut high_contrasts = HighContrastRegistry::new();
        high_contrasts.extend(high_contrast_specs);

        let simplify_specs: Vec<SimplifySpec> =
            nami_core::simplify::compile(&ext_src).unwrap_or_default();
        let simplify_names: Vec<String> =
            simplify_specs.iter().map(|s| s.name.clone()).collect();
        let mut simplifies = SimplifyRegistry::new();
        simplifies.extend(simplify_specs);

        // Collaboration pack — presence, CRDT rooms, multiplayer cursors.
        let presence_specs: Vec<PresenceSpec> =
            nami_core::presence::compile(&ext_src).unwrap_or_default();
        let presence_names: Vec<String> = if presence_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            presence_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut presences = PresenceRegistry::new();
        if presence_specs.is_empty() {
            presences.insert(PresenceSpec::default_profile());
        } else {
            presences.extend(presence_specs);
        }

        let crdt_room_specs: Vec<CrdtRoomSpec> =
            nami_core::crdt_room::compile(&ext_src).unwrap_or_default();
        let crdt_room_names: Vec<String> = if crdt_room_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            crdt_room_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut crdt_rooms = CrdtRoomRegistry::new();
        if crdt_room_specs.is_empty() {
            crdt_rooms.insert(CrdtRoomSpec::default_profile());
        } else {
            crdt_rooms.extend(crdt_room_specs);
        }

        let multiplayer_cursor_specs: Vec<MultiplayerCursorSpec> =
            nami_core::multiplayer_cursor::compile(&ext_src).unwrap_or_default();
        let multiplayer_cursor_names: Vec<String> = if multiplayer_cursor_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            multiplayer_cursor_specs
                .iter()
                .map(|s| s.name.clone())
                .collect()
        };
        let mut multiplayer_cursors = MultiplayerCursorRegistry::new();
        if multiplayer_cursor_specs.is_empty() {
            multiplayer_cursors.insert(MultiplayerCursorSpec::default_profile());
        } else {
            multiplayer_cursors.extend(multiplayer_cursor_specs);
        }

        // J2 — service workers (persistent JsRuntime + fetch interceptor).
        let service_worker_specs: Vec<ServiceWorkerSpec> =
            nami_core::service_worker::compile(&ext_src).unwrap_or_default();
        let service_worker_names: Vec<String> = if service_worker_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            service_worker_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut service_workers = ServiceWorkerRegistry::new();
        if service_worker_specs.is_empty() {
            service_workers.insert(ServiceWorkerSpec::default_profile());
        } else {
            service_workers.extend(service_worker_specs);
        }

        // (defsync) — cross-device replication channels.
        let sync_specs: Vec<SyncSpec> =
            nami_core::sync_channel::compile(&ext_src).unwrap_or_default();
        let sync_names: Vec<String> = if sync_specs.is_empty() {
            vec!["default-bookmarks".to_owned()]
        } else {
            sync_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut syncs = SyncRegistry::new();
        if sync_specs.is_empty() {
            syncs.insert(SyncSpec::default_profile());
        } else {
            syncs.extend(sync_specs);
        }

        // Tabs pack — groups, hibernation, hover previews.
        let tab_group_specs: Vec<TabGroupSpec> =
            nami_core::tab_group::compile(&ext_src).unwrap_or_default();
        let tab_group_names: Vec<String> = if tab_group_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            tab_group_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut tab_groups = TabGroupRegistry::new();
        if tab_group_specs.is_empty() {
            tab_groups.insert(TabGroupSpec::default_profile());
        } else {
            tab_groups.extend(tab_group_specs);
        }

        let tab_hibernate_specs: Vec<TabHibernateSpec> =
            nami_core::tab_hibernate::compile(&ext_src).unwrap_or_default();
        let tab_hibernate_names: Vec<String> = if tab_hibernate_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            tab_hibernate_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut tab_hibernates = TabHibernateRegistry::new();
        if tab_hibernate_specs.is_empty() {
            tab_hibernates.insert(TabHibernateSpec::default_profile());
        } else {
            tab_hibernates.extend(tab_hibernate_specs);
        }

        let tab_preview_specs: Vec<TabPreviewSpec> =
            nami_core::tab_preview::compile(&ext_src).unwrap_or_default();
        let tab_preview_names: Vec<String> = if tab_preview_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            tab_preview_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut tab_previews = TabPreviewRegistry::new();
        if tab_preview_specs.is_empty() {
            tab_previews.insert(TabPreviewSpec::default_profile());
        } else {
            tab_previews.extend(tab_preview_specs);
        }

        // Search pack — omnibox engines + !bangs.
        let search_engine_specs: Vec<SearchEngineSpec> =
            nami_core::search_engine::compile(&ext_src).unwrap_or_default();
        let search_engine_names: Vec<String> = if search_engine_specs.is_empty() {
            vec!["ddg".to_owned()]
        } else {
            search_engine_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut search_engines = SearchEngineRegistry::new();
        if search_engine_specs.is_empty() {
            search_engines.insert(SearchEngineSpec::default_profile());
        } else {
            search_engines.extend(search_engine_specs);
        }

        let search_bang_specs: Vec<SearchBangSpec> =
            nami_core::search_bang::compile(&ext_src).unwrap_or_default();
        let search_bang_triggers: Vec<String> = if search_bang_specs.is_empty() {
            vec!["g".to_owned()]
        } else {
            search_bang_specs.iter().map(|s| s.trigger.clone()).collect()
        };
        let mut search_bangs = SearchBangRegistry::new();
        if search_bang_specs.is_empty() {
            search_bangs.insert(SearchBangSpec::default_profile());
        } else {
            search_bangs.extend(search_bang_specs);
        }

        // Identity pack — multi-account personas + TOTP 2FA codes.
        let identity_specs: Vec<IdentitySpec> =
            nami_core::identity::compile(&ext_src).unwrap_or_default();
        let identity_names: Vec<String> = if identity_specs.is_empty() {
            vec!["personal".to_owned()]
        } else {
            identity_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut identities = IdentityRegistry::new();
        if identity_specs.is_empty() {
            identities.insert(IdentitySpec::default_profile());
        } else {
            identities.extend(identity_specs);
        }

        let totp_specs: Vec<TotpSpec> =
            nami_core::totp::compile(&ext_src).unwrap_or_default();
        let totp_names: Vec<String> =
            totp_specs.iter().map(|s| s.name.clone()).collect();
        let mut totps = TotpRegistry::new();
        totps.extend(totp_specs);

        // Privacy v2 pack — fingerprint farbling, cookie jars, WebGPU gating.
        let fingerprint_randomize_specs: Vec<FingerprintRandomizeSpec> =
            nami_core::fingerprint_randomize::compile(&ext_src).unwrap_or_default();
        let fingerprint_randomize_names: Vec<String> = if fingerprint_randomize_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            fingerprint_randomize_specs
                .iter()
                .map(|s| s.name.clone())
                .collect()
        };
        let mut fingerprint_randomizes = FingerprintRandomizeRegistry::new();
        if fingerprint_randomize_specs.is_empty() {
            fingerprint_randomizes.insert(FingerprintRandomizeSpec::default_profile());
        } else {
            fingerprint_randomizes.extend(fingerprint_randomize_specs);
        }

        let cookie_jar_specs: Vec<CookieJarSpec> =
            nami_core::cookie_jar::compile(&ext_src).unwrap_or_default();
        let cookie_jar_names: Vec<String> = if cookie_jar_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            cookie_jar_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut cookie_jars = CookieJarRegistry::new();
        if cookie_jar_specs.is_empty() {
            cookie_jars.insert(CookieJarSpec::default_profile());
        } else {
            cookie_jars.extend(cookie_jar_specs);
        }

        let webgpu_policy_specs: Vec<WebgpuPolicySpec> =
            nami_core::webgpu_policy::compile(&ext_src).unwrap_or_default();
        let webgpu_policy_names: Vec<String> = if webgpu_policy_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            webgpu_policy_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut webgpu_policies = WebgpuPolicyRegistry::new();
        if webgpu_policy_specs.is_empty() {
            webgpu_policies.insert(WebgpuPolicySpec::default_profile());
        } else {
            webgpu_policies.extend(webgpu_policy_specs);
        }

        // Dev pack — inspector panels, profilers, console rules.
        let inspector_specs: Vec<InspectorSpec> =
            nami_core::inspector::compile(&ext_src).unwrap_or_default();
        let inspector_names: Vec<String> =
            inspector_specs.iter().map(|s| s.name.clone()).collect();
        let mut inspectors = InspectorRegistry::new();
        inspectors.extend(inspector_specs);

        let profiler_specs: Vec<ProfilerSpec> =
            nami_core::profiler::compile(&ext_src).unwrap_or_default();
        let profiler_names: Vec<String> = if profiler_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            profiler_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut profilers = ProfilerRegistry::new();
        if profiler_specs.is_empty() {
            profilers.insert(ProfilerSpec::default_profile());
        } else {
            profilers.extend(profiler_specs);
        }

        let console_rule_specs: Vec<ConsoleRuleSpec> =
            nami_core::console_rule::compile(&ext_src).unwrap_or_default();
        let console_rule_names: Vec<String> =
            console_rule_specs.iter().map(|s| s.name.clone()).collect();
        let mut console_rules = ConsoleRuleRegistry::new();
        console_rules.extend(console_rule_specs);

        // Media pack — lock-screen session, cast receivers, subtitles.
        let media_session_specs: Vec<MediaSessionSpec> =
            nami_core::media_session::compile(&ext_src).unwrap_or_default();
        let media_session_names: Vec<String> = if media_session_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            media_session_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut media_sessions = MediaSessionRegistry::new();
        if media_session_specs.is_empty() {
            media_sessions.insert(MediaSessionSpec::default_profile());
        } else {
            media_sessions.extend(media_session_specs);
        }

        let cast_specs: Vec<CastSpec> =
            nami_core::cast::compile(&ext_src).unwrap_or_default();
        let cast_names: Vec<String> = if cast_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            cast_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut casts = CastRegistry::new();
        if cast_specs.is_empty() {
            casts.insert(CastSpec::default_profile());
        } else {
            casts.extend(cast_specs);
        }

        let subtitle_specs: Vec<SubtitleSpec> =
            nami_core::subtitle::compile(&ext_src).unwrap_or_default();
        let subtitle_names: Vec<String> = if subtitle_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            subtitle_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut subtitles = SubtitleRegistry::new();
        if subtitle_specs.is_empty() {
            subtitles.insert(SubtitleSpec::default_profile());
        } else {
            subtitles.extend(subtitle_specs);
        }

        // AI pack — providers, summarize, chat, inline completion.
        let llm_provider_specs: Vec<LlmProviderSpec> =
            nami_core::llm::compile(&ext_src).unwrap_or_default();
        let llm_provider_names: Vec<String> =
            llm_provider_specs.iter().map(|s| s.name.clone()).collect();
        let mut llm_providers = LlmProviderRegistry::new();
        llm_providers.extend(llm_provider_specs);
        let llm_engine: Arc<dyn LlmProvider> = Arc::new(EchoProvider::default());

        let summarize_specs: Vec<SummarizeSpec> =
            nami_core::summarize::compile(&ext_src).unwrap_or_default();
        let summarize_names: Vec<String> =
            summarize_specs.iter().map(|s| s.name.clone()).collect();
        let mut summarizes = SummarizeRegistry::new();
        summarizes.extend(summarize_specs);

        let chat_specs: Vec<ChatSpec> =
            nami_core::chat::compile(&ext_src).unwrap_or_default();
        let chat_names: Vec<String> =
            chat_specs.iter().map(|s| s.name.clone()).collect();
        let mut chats = ChatRegistry::new();
        chats.extend(chat_specs);

        let llm_completion_specs: Vec<LlmCompletionSpec> =
            nami_core::llm_completion::compile(&ext_src).unwrap_or_default();
        let llm_completion_names: Vec<String> = llm_completion_specs
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let mut llm_completions = LlmCompletionRegistry::new();
        llm_completions.extend(llm_completion_specs);

        // Credentials pack.
        let autofill_specs: Vec<AutofillSpec> =
            nami_core::autofill::compile(&ext_src).unwrap_or_default();
        let autofill_names: Vec<String> =
            autofill_specs.iter().map(|s| s.name.clone()).collect();
        let mut autofills = AutofillRegistry::new();
        autofills.extend(autofill_specs);

        let password_specs: Vec<PasswordsSpec> =
            nami_core::passwords::compile(&ext_src).unwrap_or_default();
        let password_names: Vec<String> =
            password_specs.iter().map(|s| s.name.clone()).collect();
        let mut passwords = PasswordsRegistry::new();
        passwords.extend(password_specs);

        let auth_saver_specs: Vec<AuthSaverSpec> =
            nami_core::auth_saver::compile(&ext_src).unwrap_or_default();
        let auth_saver_names: Vec<String> =
            auth_saver_specs.iter().map(|s| s.name.clone()).collect();
        let mut auth_savers = AuthSaverRegistry::new();
        auth_savers.extend(auth_saver_specs);

        let secure_note_specs: Vec<SecureNoteSpec> =
            nami_core::secure_note::compile(&ext_src).unwrap_or_default();
        let secure_note_names: Vec<String> =
            secure_note_specs.iter().map(|s| s.name.clone()).collect();
        let mut secure_notes = SecureNoteRegistry::new();
        secure_notes.extend(secure_note_specs);

        let passkey_specs: Vec<PasskeySpec> =
            nami_core::passkey::compile(&ext_src).unwrap_or_default();
        let passkey_names: Vec<String> =
            passkey_specs.iter().map(|s| s.name.clone()).collect();
        let mut passkeys = PasskeyRegistry::new();
        passkeys.extend(passkey_specs);

        // Mobile + download pack.
        let share_specs: Vec<ShareTargetSpec> =
            nami_core::share::compile(&ext_src).unwrap_or_default();
        let share_names: Vec<String> =
            share_specs.iter().map(|s| s.name.clone()).collect();
        let mut shares = ShareRegistry::new();
        shares.extend(share_specs);

        let offline_specs: Vec<OfflineSpec> =
            nami_core::offline::compile(&ext_src).unwrap_or_default();
        let offline_names: Vec<String> =
            offline_specs.iter().map(|s| s.name.clone()).collect();
        let mut offlines = OfflineRegistry::new();
        offlines.extend(offline_specs);

        let ptr_specs: Vec<PullRefreshSpec> =
            nami_core::pull_refresh::compile(&ext_src).unwrap_or_default();
        let pull_refresh_names: Vec<String> =
            ptr_specs.iter().map(|s| s.name.clone()).collect();
        let mut pull_refreshes = PullRefreshRegistry::new();
        pull_refreshes.extend(ptr_specs);

        let download_specs: Vec<DownloadSpec> =
            nami_core::download::compile(&ext_src).unwrap_or_default();
        let download_names: Vec<String> = if download_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            download_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut downloads = DownloadRegistry::new();
        if download_specs.is_empty() {
            downloads.insert(DownloadSpec::default_profile());
        } else {
            downloads.extend(download_specs);
        }

        // Privacy pack — spoof, dns, routing.
        let spoof_specs: Vec<SpoofSpec> =
            nami_core::spoof::compile(&ext_src).unwrap_or_default();
        let spoof_names: Vec<String> =
            spoof_specs.iter().map(|s| s.name.clone()).collect();
        let mut spoofs = SpoofRegistry::new();
        spoofs.extend(spoof_specs);

        let dns_specs: Vec<DnsSpec> =
            nami_core::dns::compile(&ext_src).unwrap_or_default();
        let dns_names: Vec<String> =
            dns_specs.iter().map(|s| s.name.clone()).collect();
        let mut dnses = DnsRegistry::new();
        dnses.extend(dns_specs);

        let routing_specs: Vec<RoutingSpec> =
            nami_core::routing::compile(&ext_src).unwrap_or_default();
        let routing_names: Vec<String> =
            routing_specs.iter().map(|s| s.name.clone()).collect();
        let mut routings = RoutingRegistry::new();
        routings.extend(routing_specs);

        // Arc pack — spaces, sidebars, splits.
        let space_specs: Vec<SpaceSpec> =
            nami_core::space::compile(&ext_src).unwrap_or_default();
        let space_names: Vec<String> =
            space_specs.iter().map(|s| s.name.clone()).collect();
        let mut spaces = SpaceRegistry::new();
        spaces.extend(space_specs);
        let space_state = Arc::new(std::sync::Mutex::new(SpaceState::new()));

        let sidebar_specs: Vec<SidebarSpec> =
            nami_core::sidebar::compile(&ext_src).unwrap_or_default();
        let sidebar_names: Vec<String> =
            sidebar_specs.iter().map(|s| s.name.clone()).collect();
        let mut sidebars = SidebarRegistry::new();
        sidebars.extend(sidebar_specs);

        let split_specs: Vec<SplitSpec> =
            nami_core::split::compile(&ext_src).unwrap_or_default();
        let split_names: Vec<String> =
            split_specs.iter().map(|s| s.name.clone()).collect();
        let mut splits = SplitRegistry::new();
        splits.extend(split_specs);

        // JS runtime specs — always include a "default" micro-eval
        // profile so POST /js/eval works out of the box.
        let js_specs: Vec<JsRuntimeSpec> =
            nami_core::js_runtime::compile(&ext_src).unwrap_or_default();
        let js_runtime_names: Vec<String> = if js_specs.is_empty() {
            vec!["default".to_owned()]
        } else {
            js_specs.iter().map(|s| s.name.clone()).collect()
        };
        let mut js_runtimes = JsRuntimeRegistry::new();
        if js_specs.is_empty() {
            js_runtimes.insert(JsRuntimeSpec::default_profile());
        } else {
            js_runtimes.extend(js_specs);
        }
        let js_engine: Arc<dyn JsRuntime> = Arc::new(MicroEval);

        // Session profile — single, default when absent. The actual
        // session store starts empty; persistence is a follow-up.
        let session_specs: Vec<SessionSpec> =
            nami_core::session::compile(&ext_src).unwrap_or_default();
        let session_spec = session_specs
            .into_iter()
            .next()
            .unwrap_or_else(SessionSpec::default_profile);
        let session_store = Arc::new(std::sync::Mutex::new(
            SessionStore::from_spec(&session_spec),
        ));

        // Security policies.
        let sp_specs: Vec<SecurityPolicySpec> =
            nami_core::security_policy::compile(&ext_src).unwrap_or_default();
        let security_policy_names: Vec<String> =
            sp_specs.iter().map(|s| s.name.clone()).collect();
        let mut security_policies = SecurityPolicyRegistry::new();
        security_policies.extend(sp_specs);

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
            "substrate loaded: {} state · {} effect · {} predicate · {} plan · {} agent · {} route · {} query · {} derived · {} component · {} transform · {} alias · {} normalize · {} wasm-agent · {} blocker · {} storage · {} extension · {} reader · {} command · {} bind · {} omnibox · {} i18n-bundles · {} security-policy · {} find · {} zoom · {} snapshot · {} pip · {} gesture · {} boost · {} js-runtime · {} space · {} sidebar · {} split · {} spoof · {} dns · {} routing · {} outline · {} annotate · {} feed · {} redirect · {} url-clean · {} script-policy · {} bridge · {} share · {} offline · {} ptr · {} download · {} autofill · {} password-vault · {} auth-saver · {} secure-note · {} passkey · {} llm-provider · {} summarize · {} chat · {} llm-completion · {} media-session · {} cast · {} subtitle · {} inspector · {} profiler · {} console-rule · {} reader-aloud · {} high-contrast · {} simplify · {} presence · {} crdt-room · {} multiplayer-cursor · {} service-worker · {} sync · {} tab-group · {} tab-hibernate · {} tab-preview · {} search-engine · {} search-bang · {} identity · {} totp · {} fingerprint-randomize · {} cookie-jar · {} webgpu-policy",
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
            messages.len(),
            security_policies.len(),
            finds.len(),
            zooms.len(),
            snapshots.len(),
            pips.len(),
            gestures.len(),
            boosts.len(),
            js_runtimes.len(),
            spaces.len(),
            sidebars.len(),
            splits.len(),
            spoofs.len(),
            dnses.len(),
            routings.len(),
            outlines.len(),
            annotates.len(),
            feeds.len(),
            redirects.len(),
            url_cleans.len(),
            script_policies.len(),
            bridges.len(),
            shares.len(),
            offlines.len(),
            pull_refreshes.len(),
            downloads.len(),
            autofills.len(),
            passwords.len(),
            auth_savers.len(),
            secure_notes.len(),
            passkeys.len(),
            llm_providers.len(),
            summarizes.len(),
            chats.len(),
            llm_completions.len(),
            media_sessions.len(),
            casts.len(),
            subtitles.len(),
            inspectors.len(),
            profilers.len(),
            console_rules.len(),
            reader_alouds.len(),
            high_contrasts.len(),
            simplifies.len(),
            presences.len(),
            crdt_rooms.len(),
            multiplayer_cursors.len(),
            service_workers.len(),
            syncs.len(),
            tab_groups.len(),
            tab_hibernates.len(),
            tab_previews.len(),
            search_engines.len(),
            search_bangs.len(),
            identities.len(),
            totps.len(),
            fingerprint_randomizes.len(),
            cookie_jars.len(),
            webgpu_policies.len(),
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
            trustdb,
            readers,
            commands,
            binds,
            omniboxes,
            messages,
            security_policies,
            finds,
            zooms,
            snapshots,
            pips,
            gestures,
            boosts,
            js_runtimes,
            js_engine,
            spaces,
            space_state,
            sidebars,
            splits,
            spoofs,
            dnses,
            routings,
            outlines,
            annotates,
            feeds,
            redirects,
            url_cleans,
            script_policies,
            bridges,
            shares,
            offlines,
            pull_refreshes,
            downloads,
            autofills,
            passwords,
            auth_savers,
            secure_notes,
            passkeys,
            llm_providers,
            llm_engine,
            summarizes,
            chats,
            llm_completions,
            media_sessions,
            casts,
            subtitles,
            inspectors,
            profilers,
            console_rules,
            reader_alouds,
            high_contrasts,
            simplifies,
            presences,
            crdt_rooms,
            multiplayer_cursors,
            service_workers,
            syncs,
            tab_groups,
            tab_hibernates,
            tab_previews,
            search_engines,
            search_bangs,
            identities,
            totps,
            fingerprint_randomizes,
            cookie_jars,
            webgpu_policies,
            session_store,
            session_spec,
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
            i18n_namespaces,
            security_policy_names,
            find_names,
            zoom_hosts,
            snapshot_names,
            pip_names,
            gesture_strokes,
            boost_names,
            js_runtime_names,
            space_names,
            sidebar_names,
            split_names,
            spoof_names,
            dns_names,
            routing_names,
            outline_names,
            annotate_names,
            feed_names,
            redirect_names,
            url_clean_names,
            script_policy_names,
            bridge_names,
            share_names,
            offline_names,
            pull_refresh_names,
            download_names,
            autofill_names,
            password_names,
            auth_saver_names,
            secure_note_names,
            passkey_names,
            llm_provider_names,
            summarize_names,
            chat_names,
            llm_completion_names,
            media_session_names,
            cast_names,
            subtitle_names,
            inspector_names,
            profiler_names,
            console_rule_names,
            reader_aloud_names,
            high_contrast_names,
            simplify_names,
            presence_names,
            crdt_room_names,
            multiplayer_cursor_names,
            service_worker_names,
            sync_names,
            tab_group_names,
            tab_hibernate_names,
            tab_preview_names,
            search_engine_names,
            search_bang_triggers,
            identity_names,
            totp_names,
            fingerprint_randomize_names,
            cookie_jar_names,
            webgpu_policy_names,
        }
    }

    // ── Accessibility-plus pack ──────────────────────────────────

    #[must_use]
    pub fn reader_aloud_list(&self) -> Vec<ReaderAloudSpec> {
        self.reader_alouds.specs().to_vec()
    }

    #[must_use]
    pub fn reader_aloud_get(&self, name: &str) -> Option<ReaderAloudSpec> {
        self.reader_alouds.get(name).cloned()
    }

    #[must_use]
    pub fn high_contrast_list(&self) -> Vec<HighContrastSpec> {
        self.high_contrasts.specs().to_vec()
    }

    #[must_use]
    pub fn high_contrast_for(&self, host: &str) -> Option<HighContrastSpec> {
        self.high_contrasts.resolve(host).cloned()
    }

    #[must_use]
    pub fn simplify_list(&self) -> Vec<SimplifySpec> {
        self.simplifies.specs().to_vec()
    }

    #[must_use]
    pub fn simplify_for(&self, host: &str) -> Option<SimplifySpec> {
        self.simplifies.resolve(host).cloned()
    }

    // ── Collaboration pack ───────────────────────────────────────

    #[must_use]
    pub fn presence_list(&self) -> Vec<PresenceSpec> {
        self.presences.specs().to_vec()
    }

    #[must_use]
    pub fn presence_for(&self, host: &str) -> Option<PresenceSpec> {
        self.presences.resolve(host).cloned()
    }

    #[must_use]
    pub fn crdt_room_list(&self) -> Vec<CrdtRoomSpec> {
        self.crdt_rooms.specs().to_vec()
    }

    #[must_use]
    pub fn crdt_room_for(&self, host: &str) -> Option<CrdtRoomSpec> {
        self.crdt_rooms.resolve(host).cloned()
    }

    #[must_use]
    pub fn multiplayer_cursor_list(&self) -> Vec<MultiplayerCursorSpec> {
        self.multiplayer_cursors.specs().to_vec()
    }

    #[must_use]
    pub fn multiplayer_cursor_for(&self, host: &str) -> Option<MultiplayerCursorSpec> {
        self.multiplayer_cursors.resolve(host).cloned()
    }

    // ── J2 service workers ───────────────────────────────────────

    #[must_use]
    pub fn service_worker_list(&self) -> Vec<ServiceWorkerSpec> {
        self.service_workers.specs().to_vec()
    }

    #[must_use]
    pub fn service_worker_for(&self, host: &str) -> Option<ServiceWorkerSpec> {
        self.service_workers.resolve(host).cloned()
    }

    // ── (defsync) cross-device replication ───────────────────────

    #[must_use]
    pub fn sync_list(&self) -> Vec<SyncSpec> {
        self.syncs.specs().to_vec()
    }

    #[must_use]
    pub fn sync_get(&self, name: &str) -> Option<SyncSpec> {
        self.syncs.get(name).cloned()
    }

    #[must_use]
    pub fn sync_for_signal(&self, signal: nami_core::sync_channel::SyncSignal) -> Vec<SyncSpec> {
        self.syncs.for_signal(signal).into_iter().cloned().collect()
    }

    // ── Tabs pack ────────────────────────────────────────────────

    #[must_use]
    pub fn tab_group_list(&self) -> Vec<TabGroupSpec> {
        self.tab_groups.specs().to_vec()
    }

    #[must_use]
    pub fn tab_group_for(&self, host: &str) -> Option<TabGroupSpec> {
        self.tab_groups.group_for_host(host).cloned()
    }

    #[must_use]
    pub fn tab_hibernate_list(&self) -> Vec<TabHibernateSpec> {
        self.tab_hibernates.specs().to_vec()
    }

    #[must_use]
    pub fn tab_hibernate_for(&self, host: &str) -> Option<TabHibernateSpec> {
        self.tab_hibernates.resolve(host).cloned()
    }

    #[must_use]
    pub fn tab_preview_list(&self) -> Vec<TabPreviewSpec> {
        self.tab_previews.specs().to_vec()
    }

    #[must_use]
    pub fn tab_preview_for(&self, host: &str) -> Option<TabPreviewSpec> {
        self.tab_previews.resolve(host).cloned()
    }

    // ── Search pack ──────────────────────────────────────────────

    #[must_use]
    pub fn search_engine_list(&self) -> Vec<SearchEngineSpec> {
        self.search_engines.specs().to_vec()
    }

    #[must_use]
    pub fn search_engine_get(&self, name: &str) -> Option<SearchEngineSpec> {
        self.search_engines.get(name).cloned()
    }

    #[must_use]
    pub fn search_engine_by_keyword(&self, keyword: &str) -> Option<SearchEngineSpec> {
        self.search_engines.by_keyword(keyword).cloned()
    }

    #[must_use]
    pub fn search_engine_default(&self) -> Option<SearchEngineSpec> {
        self.search_engines.default_engine().cloned()
    }

    #[must_use]
    pub fn search_bang_list(&self) -> Vec<SearchBangSpec> {
        self.search_bangs.specs().to_vec()
    }

    #[must_use]
    pub fn search_bang_detect(&self, input: &str) -> Option<(SearchBangSpec, String)> {
        self.search_bangs
            .detect(input)
            .map(|m| (m.spec.clone(), m.remaining.to_owned()))
    }

    // ── Identity pack ────────────────────────────────────────────

    #[must_use]
    pub fn identity_list(&self) -> Vec<IdentitySpec> {
        self.identities.specs().to_vec()
    }

    #[must_use]
    pub fn identity_get(&self, name: &str) -> Option<IdentitySpec> {
        self.identities.get(name).cloned()
    }

    #[must_use]
    pub fn identity_for(&self, host: &str) -> Option<IdentitySpec> {
        self.identities.resolve(host).cloned()
    }

    #[must_use]
    pub fn totp_list(&self) -> Vec<TotpSpec> {
        self.totps.specs().to_vec()
    }

    #[must_use]
    pub fn totp_get(&self, name: &str) -> Option<TotpSpec> {
        self.totps.get(name).cloned()
    }

    #[must_use]
    pub fn totp_for_identity(&self, identity: &str) -> Vec<TotpSpec> {
        self.totps.for_identity(identity).into_iter().cloned().collect()
    }

    /// Generate the current TOTP code for a named profile.
    pub fn totp_code(&self, name: &str) -> Option<Result<String, String>> {
        self.totps
            .get(name)
            .map(|s| s.generate_now().map_err(|e| e.to_string()))
    }

    // ── Privacy v2 pack ──────────────────────────────────────────

    #[must_use]
    pub fn fingerprint_randomize_list(&self) -> Vec<FingerprintRandomizeSpec> {
        self.fingerprint_randomizes.specs().to_vec()
    }

    #[must_use]
    pub fn fingerprint_randomize_for(&self, host: &str) -> Option<FingerprintRandomizeSpec> {
        self.fingerprint_randomizes.resolve(host).cloned()
    }

    #[must_use]
    pub fn cookie_jar_list(&self) -> Vec<CookieJarSpec> {
        self.cookie_jars.specs().to_vec()
    }

    #[must_use]
    pub fn cookie_jar_for(&self, host: &str) -> Option<CookieJarSpec> {
        self.cookie_jars.resolve(host).cloned()
    }

    #[must_use]
    pub fn webgpu_policy_list(&self) -> Vec<WebgpuPolicySpec> {
        self.webgpu_policies.specs().to_vec()
    }

    #[must_use]
    pub fn webgpu_policy_for(&self, host: &str) -> Option<WebgpuPolicySpec> {
        self.webgpu_policies.resolve(host).cloned()
    }

    // ── Dev pack ─────────────────────────────────────────────────

    #[must_use]
    pub fn inspector_list(&self) -> Vec<InspectorSpec> {
        self.inspectors.specs().to_vec()
    }

    #[must_use]
    pub fn inspector_get(&self, name: &str) -> Option<InspectorSpec> {
        self.inspectors.get(name).cloned()
    }

    #[must_use]
    pub fn inspector_visible(&self) -> Vec<InspectorSpec> {
        self.inspectors.visible().into_iter().cloned().collect()
    }

    #[must_use]
    pub fn profiler_list(&self) -> Vec<ProfilerSpec> {
        self.profilers.specs().to_vec()
    }

    #[must_use]
    pub fn profiler_get(&self, name: &str) -> Option<ProfilerSpec> {
        self.profilers.get(name).cloned()
    }

    #[must_use]
    pub fn console_rule_list(&self) -> Vec<ConsoleRuleSpec> {
        self.console_rules.specs().to_vec()
    }

    // ── Media pack ───────────────────────────────────────────────

    #[must_use]
    pub fn media_session_list(&self) -> Vec<MediaSessionSpec> {
        self.media_sessions.specs().to_vec()
    }

    #[must_use]
    pub fn media_session_for(&self, host: &str) -> Option<MediaSessionSpec> {
        self.media_sessions.resolve(host).cloned()
    }

    #[must_use]
    pub fn cast_list(&self) -> Vec<CastSpec> {
        self.casts.specs().to_vec()
    }

    #[must_use]
    pub fn cast_applicable(&self, host: &str) -> Vec<CastSpec> {
        self.casts.applicable(host).into_iter().cloned().collect()
    }

    #[must_use]
    pub fn subtitle_list(&self) -> Vec<SubtitleSpec> {
        self.subtitles.specs().to_vec()
    }

    #[must_use]
    pub fn subtitle_for(&self, host: &str) -> Option<SubtitleSpec> {
        self.subtitles.resolve(host).cloned()
    }

    // ── AI pack ──────────────────────────────────────────────────

    #[must_use]
    pub fn llm_provider_list(&self) -> Vec<LlmProviderSpec> {
        self.llm_providers.specs().to_vec()
    }

    #[must_use]
    pub fn llm_provider_get(&self, name: &str) -> Option<LlmProviderSpec> {
        self.llm_providers.get(name).cloned()
    }

    #[must_use]
    pub fn llm_engine_name(&self) -> &'static str {
        self.llm_engine.engine_name()
    }

    #[must_use]
    pub fn summarize_list(&self) -> Vec<SummarizeSpec> {
        self.summarizes.specs().to_vec()
    }

    #[must_use]
    pub fn summarize_get(&self, name: &str) -> Option<SummarizeSpec> {
        self.summarizes.get(name).cloned()
    }

    /// Run a summarize profile against source text.
    pub fn summarize_run(
        &self,
        name: &str,
        source: &str,
    ) -> Result<nami_core::llm::LlmResponse, nami_core::llm::LlmError> {
        let spec = self.summarizes.get(name).cloned().ok_or_else(|| {
            nami_core::llm::LlmError::InvalidSpec(format!("no summarize '{name}'"))
        })?;
        let provider_spec = self
            .llm_providers
            .get(&spec.provider)
            .cloned()
            .ok_or_else(|| {
                nami_core::llm::LlmError::InvalidSpec(format!(
                    "no llm provider '{}'",
                    spec.provider
                ))
            })?;
        spec.run(&*self.llm_engine, &provider_spec, source)
    }

    #[must_use]
    pub fn chat_list(&self) -> Vec<ChatSpec> {
        self.chats.specs().to_vec()
    }

    #[must_use]
    pub fn chat_get(&self, name: &str) -> Option<ChatSpec> {
        self.chats.get(name).cloned()
    }

    /// Ask a chat profile a question against a page-context string.
    pub fn chat_ask(
        &self,
        name: &str,
        page_context: Option<&str>,
        history: &[nami_core::llm::LlmMessage],
        question: &str,
    ) -> Result<nami_core::llm::LlmResponse, nami_core::llm::LlmError> {
        let spec = self.chats.get(name).cloned().ok_or_else(|| {
            nami_core::llm::LlmError::InvalidSpec(format!("no chat '{name}'"))
        })?;
        let provider_spec = self
            .llm_providers
            .get(&spec.provider)
            .cloned()
            .ok_or_else(|| {
                nami_core::llm::LlmError::InvalidSpec(format!(
                    "no llm provider '{}'",
                    spec.provider
                ))
            })?;
        spec.run(&*self.llm_engine, &provider_spec, page_context, history, question)
    }

    #[must_use]
    pub fn llm_completion_list(&self) -> Vec<LlmCompletionSpec> {
        self.llm_completions.specs().to_vec()
    }

    /// Run a completion profile against `prefix`.
    pub fn llm_completion_run(
        &self,
        name: &str,
        prefix: &str,
    ) -> Result<nami_core::llm::LlmResponse, nami_core::llm::LlmError> {
        let spec = self.llm_completions.get(name).cloned().ok_or_else(|| {
            nami_core::llm::LlmError::InvalidSpec(format!(
                "no llm-completion '{name}'"
            ))
        })?;
        let provider_spec = self
            .llm_providers
            .get(&spec.provider)
            .cloned()
            .ok_or_else(|| {
                nami_core::llm::LlmError::InvalidSpec(format!(
                    "no llm provider '{}'",
                    spec.provider
                ))
            })?;
        spec.run(&*self.llm_engine, &provider_spec, prefix)
    }

    // ── Credentials pack ─────────────────────────────────────────

    #[must_use]
    pub fn autofill_list(&self) -> Vec<AutofillSpec> {
        self.autofills.specs().to_vec()
    }

    #[must_use]
    pub fn autofill_get(&self, name: &str) -> Option<AutofillSpec> {
        self.autofills.get(name).cloned()
    }

    #[must_use]
    pub fn password_list(&self) -> Vec<PasswordsSpec> {
        self.passwords.specs().to_vec()
    }

    #[must_use]
    pub fn password_get(&self, name: &str) -> Option<PasswordsSpec> {
        self.passwords.get(name).cloned()
    }

    /// Every password vault whose auto-fill scope covers `host`.
    #[must_use]
    pub fn passwords_for(&self, host: &str) -> Vec<PasswordsSpec> {
        self.passwords.applicable(host).into_iter().cloned().collect()
    }

    #[must_use]
    pub fn auth_saver_list(&self) -> Vec<AuthSaverSpec> {
        self.auth_savers.specs().to_vec()
    }

    #[must_use]
    pub fn auth_saver_for(&self, host: &str) -> Option<AuthSaverSpec> {
        self.auth_savers.resolve(host).cloned()
    }

    #[must_use]
    pub fn secure_note_list(&self) -> Vec<SecureNoteSpec> {
        self.secure_notes.specs().to_vec()
    }

    #[must_use]
    pub fn passkey_list(&self) -> Vec<PasskeySpec> {
        self.passkeys.specs().to_vec()
    }

    /// Every passkey profile that permits `rp_id`.
    #[must_use]
    pub fn passkeys_for(&self, rp_id: &str) -> Vec<PasskeySpec> {
        self.passkeys
            .applicable(rp_id)
            .into_iter()
            .cloned()
            .collect()
    }

    // ── Mobile + download pack ───────────────────────────────────

    #[must_use]
    pub fn share_list(&self) -> Vec<ShareTargetSpec> {
        self.shares.specs().to_vec()
    }

    #[must_use]
    pub fn offline_list(&self) -> Vec<OfflineSpec> {
        self.offlines.specs().to_vec()
    }

    #[must_use]
    pub fn pull_refresh_list(&self) -> Vec<PullRefreshSpec> {
        self.pull_refreshes.specs().to_vec()
    }

    #[must_use]
    pub fn pull_refresh_for(&self, host: &str) -> Option<PullRefreshSpec> {
        self.pull_refreshes.resolve(host).cloned()
    }

    #[must_use]
    pub fn download_list(&self) -> Vec<DownloadSpec> {
        self.downloads.specs().to_vec()
    }

    #[must_use]
    pub fn download_get(&self, name: &str) -> Option<DownloadSpec> {
        self.downloads.get(name).cloned()
    }

    // ── Reading pack ─────────────────────────────────────────────

    #[must_use]
    pub fn outline_profile(&self, name: Option<&str>) -> OutlineSpec {
        name.and_then(|n| self.outlines.get(n))
            .cloned()
            .unwrap_or_else(OutlineSpec::default_profile)
    }

    #[must_use]
    pub fn outline_extract(
        &self,
        doc: &Document,
        name: Option<&str>,
    ) -> Vec<nami_core::outline::OutlineEntry> {
        let spec = self.outline_profile(name);
        nami_core::outline::extract_outline(doc, &spec)
    }

    #[must_use]
    pub fn annotate_list(&self) -> Vec<AnnotateSpec> {
        self.annotates.specs().to_vec()
    }

    #[must_use]
    pub fn annotate_get(&self, name: &str) -> Option<AnnotateSpec> {
        self.annotates.get(name).cloned()
    }

    #[must_use]
    pub fn feed_list(&self) -> Vec<FeedSpec> {
        self.feeds.specs().to_vec()
    }

    #[must_use]
    pub fn feed_get(&self, name: &str) -> Option<FeedSpec> {
        self.feeds.get(name).cloned()
    }

    // ── TOR-v2 pack ──────────────────────────────────────────────

    #[must_use]
    pub fn redirect_list(&self) -> Vec<RedirectSpec> {
        self.redirects.specs().to_vec()
    }

    #[must_use]
    pub fn redirect_apply(&self, input_url: &str) -> Option<String> {
        let parsed = url::Url::parse(input_url).ok()?;
        let host = parsed.host_str().unwrap_or("");
        self.redirects.resolve(host)
            .and_then(|spec| spec.rewrite(input_url, 0))
    }

    #[must_use]
    pub fn url_clean_list(&self) -> Vec<UrlCleanSpec> {
        self.url_cleans.specs().to_vec()
    }

    #[must_use]
    pub fn url_clean_apply(&self, input_url: &str) -> String {
        self.url_cleans.apply(input_url)
    }

    #[must_use]
    pub fn script_policy_list(&self) -> Vec<ScriptPolicySpec> {
        self.script_policies.specs().to_vec()
    }

    #[must_use]
    pub fn script_policy_for(&self, host: &str) -> Option<ScriptPolicySpec> {
        self.script_policies.resolve(host).cloned()
    }

    #[must_use]
    pub fn bridge_list(&self) -> Vec<BridgeSpec> {
        self.bridges.specs().to_vec()
    }

    #[must_use]
    pub fn bridge_get(&self, name: &str) -> Option<BridgeSpec> {
        self.bridges.get(name).cloned()
    }

    #[must_use]
    pub fn bridges_torrc_block(&self) -> String {
        self.bridges.to_torrc_block()
    }

    // ── Privacy-pack accessors ───────────────────────────────────

    #[must_use]
    pub fn spoofs_list(&self) -> Vec<SpoofSpec> {
        self.spoofs.specs().to_vec()
    }

    #[must_use]
    pub fn spoof_for(&self, host: &str) -> Option<SpoofSpec> {
        self.spoofs.resolve(host).cloned()
    }

    #[must_use]
    pub fn dns_list(&self) -> Vec<DnsSpec> {
        self.dnses.specs().to_vec()
    }

    #[must_use]
    pub fn dns_get(&self, name: &str) -> Option<DnsSpec> {
        self.dnses.get(name).cloned()
    }

    #[must_use]
    pub fn routing_list(&self) -> Vec<RoutingSpec> {
        self.routings.specs().to_vec()
    }

    /// Resolve the active route for `host`. Returns `(spec_name, RouteVia)`
    /// pair — name is `None` when falling through to direct default.
    #[must_use]
    pub fn routing_for(&self, host: &str) -> (Option<String>, RouteVia) {
        match self.routings.resolve(host) {
            Some(spec) => (Some(spec.name.clone()), spec.parsed_via()),
            None => (None, RouteVia::Direct),
        }
    }

    // ── Arc-pack accessors ───────────────────────────────────────

    #[must_use]
    pub fn spaces_list(&self) -> Vec<SpaceSpec> {
        self.spaces.specs().to_vec()
    }

    #[must_use]
    pub fn space_get(&self, name: &str) -> Option<SpaceSpec> {
        self.spaces.get(name).cloned()
    }

    pub fn space_activate(&self, name: &str) -> bool {
        if self.spaces.get(name).is_none() {
            return false;
        }
        let Ok(mut st) = self.space_state.lock() else {
            return false;
        };
        st.activate(name);
        true
    }

    pub fn space_deactivate(&self) {
        if let Ok(mut st) = self.space_state.lock() {
            st.deactivate();
        }
    }

    #[must_use]
    pub fn space_active(&self) -> Option<String> {
        self.space_state
            .lock()
            .ok()
            .and_then(|st| st.active().map(str::to_owned))
    }

    #[must_use]
    pub fn sidebars_list(&self) -> Vec<SidebarSpec> {
        self.sidebars.specs().to_vec()
    }

    #[must_use]
    pub fn sidebars_visible(&self, host: &str) -> Vec<SidebarSpec> {
        let active = self.space_active();
        self.sidebars
            .visible(active.as_deref(), host)
            .into_iter()
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn splits_list(&self) -> Vec<SplitSpec> {
        self.splits.specs().to_vec()
    }

    #[must_use]
    pub fn split_get(&self, name: &str) -> Option<SplitSpec> {
        self.splits.get(name).cloned()
    }

    /// Names of every declared JsRuntime profile.
    #[must_use]
    pub fn js_runtime_names(&self) -> &[String] {
        &self.js_runtime_names
    }

    /// Resolve a runtime profile by name, falling back to the first
    /// installed spec. Returns `None` only when the registry is empty
    /// (which doesn't happen in practice — a default always loads).
    #[must_use]
    pub fn js_runtime_profile(&self, name: Option<&str>) -> Option<JsRuntimeSpec> {
        name.and_then(|n| self.js_runtimes.get(n))
            .or_else(|| self.js_runtimes.specs().first())
            .cloned()
    }

    /// Run the active engine under a named profile. Caller's `vars`
    /// passthrough lets boost scripts read host-provided values.
    pub fn js_eval(
        &self,
        source: &str,
        profile: Option<&str>,
        vars: HashMap<String, nami_core::js_runtime::Value>,
        origin: Option<String>,
    ) -> Result<ExecutionResult, EvalError> {
        let spec = self
            .js_runtime_profile(profile)
            .ok_or_else(|| EvalError::Runtime("no JS runtime installed".into()))?;
        let ctx = EvalContext { vars, origin };
        self.js_engine.eval(source, &spec, &ctx)
    }

    #[must_use]
    pub fn js_engine_name(&self) -> &'static str {
        self.js_engine.engine_name()
    }

    // ── Tier-1 accessor surface ───────────────────────────────────

    #[must_use]
    pub fn find_profile(&self, name: Option<&str>) -> FindSpec {
        name.and_then(|n| self.finds.get(n))
            .cloned()
            .unwrap_or_else(FindSpec::default_profile)
    }

    #[must_use]
    pub fn find_in(&self, doc: &Document, query: &str, profile: Option<&str>) -> Vec<nami_core::find::FindMatch> {
        let spec = self.find_profile(profile);
        nami_core::find::find_in_document(doc, query, &spec)
    }

    #[must_use]
    pub fn zoom_for(&self, host: &str) -> (f32, bool) {
        (
            self.zooms.level_for(host),
            self.zooms.text_only_for(host),
        )
    }

    #[must_use]
    pub fn snapshot_recipe(&self, name: Option<&str>, host: &str) -> Option<SnapshotSpec> {
        name.and_then(|n| self.snapshots.get(n))
            .or_else(|| self.snapshots.resolve(host))
            .cloned()
    }

    #[must_use]
    pub fn pip_for(&self, host: &str) -> Option<PipSpec> {
        self.pips.resolve(host).cloned()
    }

    #[must_use]
    pub fn gesture_dispatch(&self, stroke: &str) -> Option<GestureSpec> {
        self.gestures.resolve(stroke).cloned()
    }

    /// Every boost's CSS merged for this host.
    #[must_use]
    pub fn boost_css(&self, host: &str) -> String {
        self.boosts.merged_css(host)
    }

    /// Extra blocker selectors contributed by boosts for this host.
    #[must_use]
    pub fn boost_blocker_selectors(&self, host: &str) -> Vec<String> {
        self.boosts.merged_blocker_selectors(host)
    }

    /// Full boost-spec list applicable to this host (after the enabled gate).
    #[must_use]
    pub fn boosts_applicable(&self, host: &str) -> Vec<BoostSpec> {
        self.boosts
            .applicable(host)
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn boost_set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        self.boosts.set_enabled(name, enabled)
    }

    #[must_use]
    pub fn session_spec(&self) -> &SessionSpec {
        &self.session_spec
    }

    pub fn session_record_open(&self, rec: TabRecord) {
        if let Ok(mut s) = self.session_store.lock() {
            s.record_open(rec);
        }
    }

    pub fn session_record_close(&self, rec: TabRecord) {
        if let Ok(mut s) = self.session_store.lock() {
            s.record_close(rec);
        }
    }

    pub fn session_undo_close(&self) -> Option<TabRecord> {
        self.session_store.lock().ok()?.undo_close()
    }

    #[must_use]
    pub fn session_closed_tabs(&self) -> Vec<TabRecord> {
        self.session_store
            .lock()
            .map(|s| s.closed_tabs())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn session_snapshot(&self) -> Vec<TabRecord> {
        self.session_store
            .lock()
            .map(|s| s.snapshot())
            .unwrap_or_default()
    }

    pub fn session_restore(&self, tabs: Vec<TabRecord>) {
        if let Ok(mut s) = self.session_store.lock() {
            s.restore(tabs);
        }
    }

    /// Range scan over a declared secondary index.
    #[must_use]
    pub fn storage_by_index_range(
        &self,
        store: &str,
        path: &str,
        lo: &str,
        hi: &str,
    ) -> Option<Vec<(String, serde_json::Value)>> {
        self.get_store(store)?.by_index_range(path, lo, hi)
    }

    /// i18n lookup. Returns the translated string or (per fallback chain)
    /// the raw key.
    #[must_use]
    pub fn i18n_get(&self, namespace: &str, locale: &str, key: &str) -> String {
        self.messages.get(namespace, locale, key)
    }

    /// i18n namespaces currently installed.
    #[must_use]
    pub fn i18n_namespaces(&self) -> &[String] {
        &self.i18n_namespaces
    }

    /// Every locale present under `namespace`, sorted.
    #[must_use]
    pub fn i18n_locales(&self, namespace: &str) -> Vec<String> {
        self.messages.locales_for(namespace)
    }

    /// Translation-coverage diagnostic — keys in :en missing from :locale.
    #[must_use]
    pub fn i18n_missing(&self, namespace: &str, locale: &str) -> Vec<String> {
        self.messages.missing(namespace, locale)
    }

    /// Security-policy headers for `host`. Empty when no rule matches.
    #[must_use]
    pub fn security_policy_headers(&self, host: &str) -> PolicyHeaders {
        self.security_policies.headers_for(host)
    }

    /// The matching SecurityPolicySpec for inspection.
    #[must_use]
    pub fn security_policy_for(&self, host: &str) -> Option<SecurityPolicySpec> {
        self.security_policies.resolve(host).cloned()
    }

    /// Every installed security-policy's name.
    #[must_use]
    pub fn security_policy_names(&self) -> &[String] {
        &self.security_policy_names
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

    /// Verify a signed extension bundle against the trust DB. Returns
    /// the full VerificationStatus so callers can distinguish Trusted
    /// from ValidButUntrusted (e.g., for a TOFU install prompt).
    pub fn verify_signed_extension(
        &self,
        signed: &SignedExtension,
    ) -> Result<VerificationStatus, VerificationError> {
        let db = self.trustdb.lock().map_err(|_| {
            VerificationError::Canonicalize("trustdb mutex poisoned".into())
        })?;
        nami_core::extension::verify(signed, &db)
    }

    /// Trust a new pubkey (base64-encoded ed25519). Does not persist
    /// to disk — caller is responsible for rewriting trustdb.txt.
    pub fn trust_pubkey(&self, pubkey_b64: &str) -> bool {
        let Ok(mut db) = self.trustdb.lock() else {
            return false;
        };
        db.trust(pubkey_b64.to_owned());
        true
    }

    /// Revoke a previously trusted pubkey.
    pub fn revoke_pubkey(&self, pubkey_b64: &str) -> bool {
        let Ok(mut db) = self.trustdb.lock() else {
            return false;
        };
        db.revoke(pubkey_b64)
    }

    /// Every trusted pubkey, sorted. Powers /trustdb.
    #[must_use]
    pub fn trustdb_keys(&self) -> Vec<String> {
        self.trustdb
            .lock()
            .map(|db| db.keys())
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
            i18n: self.i18n_namespaces.clone(),
            security_policies: self.security_policy_names.clone(),
            finds: self.find_names.clone(),
            zooms: self.zoom_hosts.clone(),
            snapshots: self.snapshot_names.clone(),
            pips: self.pip_names.clone(),
            gestures: self.gesture_strokes.clone(),
            boosts: self.boost_names.clone(),
            js_runtimes: self.js_runtime_names.clone(),
            spaces: self.space_names.clone(),
            sidebars: self.sidebar_names.clone(),
            splits: self.split_names.clone(),
            spoofs: self.spoof_names.clone(),
            dnses: self.dns_names.clone(),
            routings: self.routing_names.clone(),
            outlines: self.outline_names.clone(),
            annotates: self.annotate_names.clone(),
            feeds: self.feed_names.clone(),
            redirects: self.redirect_names.clone(),
            url_cleans: self.url_clean_names.clone(),
            script_policies: self.script_policy_names.clone(),
            bridges: self.bridge_names.clone(),
            shares: self.share_names.clone(),
            offlines: self.offline_names.clone(),
            pull_refreshes: self.pull_refresh_names.clone(),
            downloads: self.download_names.clone(),
            autofills: self.autofill_names.clone(),
            passwords: self.password_names.clone(),
            auth_savers: self.auth_saver_names.clone(),
            secure_notes: self.secure_note_names.clone(),
            passkeys: self.passkey_names.clone(),
            llm_providers: self.llm_provider_names.clone(),
            summarizes: self.summarize_names.clone(),
            chats: self.chat_names.clone(),
            llm_completions: self.llm_completion_names.clone(),
            media_sessions: self.media_session_names.clone(),
            casts: self.cast_names.clone(),
            subtitles: self.subtitle_names.clone(),
            inspectors: self.inspector_names.clone(),
            profilers: self.profiler_names.clone(),
            console_rules: self.console_rule_names.clone(),
            reader_alouds: self.reader_aloud_names.clone(),
            high_contrasts: self.high_contrast_names.clone(),
            simplifies: self.simplify_names.clone(),
            presences: self.presence_names.clone(),
            crdt_rooms: self.crdt_room_names.clone(),
            multiplayer_cursors: self.multiplayer_cursor_names.clone(),
            service_workers: self.service_worker_names.clone(),
            syncs: self.sync_names.clone(),
            tab_groups: self.tab_group_names.clone(),
            tab_hibernates: self.tab_hibernate_names.clone(),
            tab_previews: self.tab_preview_names.clone(),
            search_engines: self.search_engine_names.clone(),
            search_bangs: self.search_bang_triggers.clone(),
            identities: self.identity_names.clone(),
            totps: self.totp_names.clone(),
            fingerprint_randomizes: self.fingerprint_randomize_names.clone(),
            cookie_jars: self.cookie_jar_names.clone(),
            webgpu_policies: self.webgpu_policy_names.clone(),
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
