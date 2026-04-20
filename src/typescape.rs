//! namimado typescape — the browser's contribution to the arch-
//! synthesizer Merkle tree. Composes nami-core's leaf manifest with
//! namimado-specific additions: shipped rule packs, HTTP endpoints,
//! MCP tools. BLAKE3-attestable.
//!
//! The goal is "prove everything about nami in the abstract" — one
//! queryable structure from which tooling, agents, and the UI can
//! reason about what the binary actually does.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Every shipped normalize pack, discovered at compile time via
/// `include_str!`. The content itself doesn't live in the typescape
/// (it's already in the source tree) — we only record the file name
/// so callers can introspect.
const PACK_FILES: &[&str] = &[
    "html5.lisp",
    "shadcn.lisp",
    "shadcn-emit.lisp",
    "mui.lisp",
    "bootstrap.lisp",
    "tailwind.lisp",
    "blocker-trackers.lisp",
    "vim-mode.lisp",
];

/// namimado typescape — namimado's dimensions plus the embedded
/// nami-core leaf.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NamimadoTypescape {
    pub name: String,
    pub version: String,
    /// Content hash of the combined (nami-core + namimado) manifest.
    pub hash: String,

    /// Packs shipped with this binary (file names, path-relative).
    pub normalize_packs: Vec<NormalizePackInfo>,
    /// HTTP endpoints exposed when `serve` is active.
    pub http_endpoints: Vec<HttpEndpointInfo>,
    /// MCP tools exposed when `mcp` is active.
    pub mcp_tools: Vec<McpToolInfo>,
    /// Feature flags at compile time.
    pub features: Vec<String>,

    /// The embedded nami-core typescape — same structure
    /// arch-synthesizer's aggregator reads. Stored as a generic JSON
    /// `Value` here so this outer struct can impl `JsonSchema`
    /// without requiring the upstream leaf to.
    pub nami_core: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NormalizePackInfo {
    pub file: String,
    pub kind: PackKind,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum PackKind {
    /// Inbound: framework-specific → canonical `n-*`.
    Inbound,
    /// Outbound: canonical `n-*` → framework-specific shape.
    Emit,
    /// Content blocking: domain + selector rules stripping trackers.
    Blocker,
    /// Keyboard pack: (defcommand) + (defbind) forms.
    Keybindings,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HttpEndpointInfo {
    pub method: String,
    pub path: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
}

/// Build the manifest.
pub fn typescape() -> NamimadoTypescape {
    #[cfg(feature = "browser-core")]
    let nami_core = serde_json::to_value(nami_core::typescape::typescape())
        .unwrap_or(serde_json::Value::Null);
    #[cfg(not(feature = "browser-core"))]
    let nami_core = serde_json::Value::Null;

    let ts = NamimadoTypescape {
        name: "namimado".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        hash: String::new(), // filled below
        normalize_packs: normalize_packs(),
        http_endpoints: http_endpoints(),
        mcp_tools: mcp_tools(),
        features: features(),
        nami_core,
    };
    // Hash over the zero-hash version so the hash itself is
    // content-determined + deterministic across rebuilds.
    let to_hash = NamimadoTypescape {
        hash: String::new(),
        ..ts.clone()
    };
    let json = serde_json::to_vec(&to_hash).expect("typescape serializes");
    let h = blake3::hash(&json);
    NamimadoTypescape {
        hash: base32_16(&h.as_bytes()[..16]),
        ..ts
    }
}

/// 128 bits of BLAKE3 → 26-char base32 lowercase — matches
/// nami-core's convention and the wider pleme-io attestation chain.
fn base32_16(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for &b in bytes {
        buf = (buf << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buf >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buf << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

fn normalize_packs() -> Vec<NormalizePackInfo> {
    let mk = |file: &str, kind: PackKind, desc: &str| NormalizePackInfo {
        file: file.to_owned(),
        kind,
        description: desc.to_owned(),
    };
    let mut packs: Vec<NormalizePackInfo> = PACK_FILES
        .iter()
        .map(|f| match *f {
            "html5.lisp" => mk(f, PackKind::Inbound, "HTML5 semantic tags → n-*"),
            "shadcn.lisp" => mk(f, PackKind::Inbound, "shadcn/radix data-slot idioms → n-*"),
            "shadcn-emit.lisp" => mk(f, PackKind::Emit, "n-* → shadcn-shaped DOM"),
            "mui.lisp" => mk(f, PackKind::Inbound, "MUI component root classes → n-*"),
            "bootstrap.lisp" => mk(f, PackKind::Inbound, "Bootstrap component classes → n-*"),
            "tailwind.lisp" => mk(f, PackKind::Inbound, "Tailwind utility-class patterns → n-*"),
            "blocker-trackers.lisp" => mk(f, PackKind::Blocker, "Third-party trackers + ad-network blocklist"),
            "vim-mode.lisp" => mk(f, PackKind::Keybindings, "Vim-mode defaults — modal navigation, scroll, reader/blocker toggles"),
            _ => mk(f, PackKind::Inbound, "(undocumented)"),
        })
        .collect();
    packs.sort_by(|a, b| a.file.cmp(&b.file));
    packs
}

fn http_endpoints() -> Vec<HttpEndpointInfo> {
    let mk = |m: &str, p: &str, d: &str| HttpEndpointInfo {
        method: m.to_owned(),
        path: p.to_owned(),
        description: d.to_owned(),
    };
    vec![
        mk("GET", "/status", "Liveness + feature set + loaded_at + reload_count."),
        mk("POST", "/navigate", "Navigate URL through the full Lisp substrate pipeline."),
        mk("GET", "/report", "Structured substrate report from the last navigate."),
        mk("GET", "/state", "Current state-store snapshot."),
        mk("GET", "/dom", "Last navigated page as S-expression."),
        mk("GET", "/rules", "Full DSL rule inventory by keyword."),
        mk("POST", "/reload", "Re-scan rc files + substrate.d/; swap pipeline."),
        mk("GET", "/openapi.yaml", "This control API as OpenAPI 3.0.3 YAML."),
        mk("GET", "/openapi.json", "OpenAPI spec as JSON."),
        mk("GET", "/typescape", "This typescape manifest (self-describing)."),
        mk("GET", "/ui", "Embedded inspector SPA."),
        mk("GET", "/accessibility", "ARIA accessibility tree of the last navigated page (canonical n-* IS the role map)."),
        mk("GET", "/theme", "Current irodzuki ColorScheme as JSON — palette the GPU window + /ui both inherit."),
        mk("GET", "/theme.css", "Same scheme as CSS custom properties (`:root { --bg: …; --fg: …; --accent: …; }`) — the inspector SPA links this so a scheme swap propagates without a rebuild."),
        mk("GET", "/history", "Browsing history; `?q=` searches, `?limit=N` recent."),
        mk("DELETE", "/history", "Clear all history."),
        mk("GET", "/bookmarks", "List all bookmarks."),
        mk("POST", "/bookmarks", "Add a bookmark."),
        mk("DELETE", "/bookmarks", "Remove a bookmark (`?url=…`)."),
        mk("GET", "/storage", "Per-store summary of every (defstorage …) declared store."),
        mk("GET", "/storage/:name", "Full entry snapshot for one store (or single value with `?key=…`)."),
        mk("POST", "/storage/:name", "Write one key→value into a store (body `{key,value}`)."),
        mk("DELETE", "/storage/:name", "Delete one key from a store (`?key=…`)."),
        mk("GET", "/storage/:name/index", "List declared indexes + distinct projected values."),
        mk("GET", "/storage/:name/index/:path", "Entries whose value at `path` equals `?value=…` (O(log n))."),
        mk("GET", "/storage/:name/index/:path/range", "Range scan over a secondary index (`?lo=&hi=`, inclusive)."),
        mk("GET", "/i18n/:namespace", "Resolve one translated message (`?locale=&key=`); applies fallback chain."),
        mk("GET", "/i18n/:namespace/coverage", "Translation coverage — available locales + missing keys vs :en."),
        mk("GET", "/security-policy", "Render per-host security-policy headers (`?host=…`) via (defsecurity-policy)."),
        mk("POST", "/find", "Find-in-page against the last-navigated doc (body: query + optional profile)."),
        mk("GET", "/zoom", "Resolved zoom + text-only flag for `?host=…`."),
        mk("GET", "/snapshot/recipe", "Snapshot recipe for `?host=…&name=…` (pixel capture is GPU-side)."),
        mk("GET", "/pip", "Picture-in-picture rule for `?host=…`."),
        mk("POST", "/gesture/dispatch", "Dispatch a mouse-gesture stroke → command."),
        mk("GET", "/boosts", "Per-host (or all) (defboost) overlay inventory."),
        mk("GET", "/boosts/css", "Merged CSS from every applicable boost for `?host=…`."),
        mk("POST", "/boosts/:name/enabled", "Toggle a boost at runtime."),
        mk("GET", "/session/open", "Currently-open session tabs."),
        mk("GET", "/session/closed", "Recently-closed tabs, newest first."),
        mk("POST", "/session/undo-close", "Pop the most-recently-closed tab (Cmd+Shift+T)."),
        mk("POST", "/js/eval", "Evaluate JS-ish source through the active (defjs-runtime) engine."),
        mk("GET", "/spaces", "Every (defspace) declaration."),
        mk("GET", "/spaces/active", "Currently active space name (or null)."),
        mk("DELETE", "/spaces/active", "Deactivate — clear the active-space slot."),
        mk("GET", "/spaces/:name", "Full SpaceSpec for one space."),
        mk("POST", "/spaces/:name/activate", "Activate a space."),
        mk("GET", "/sidebars", "List (defsidebar) apps; `?host=…` filters to visible-under."),
        mk("GET", "/splits", "Every (defsplit) layout."),
        mk("GET", "/splits/:name", "Full SplitSpec for one layout."),
        mk("GET", "/spoofs", "Every (defspoof) fingerprint-resistance profile."),
        mk("GET", "/spoof", "Resolved (defspoof) for `?host=…` (or 404)."),
        mk("GET", "/dns", "Every (defdns) resolver profile."),
        mk("GET", "/dns/:name", "Full DnsSpec for one resolver profile."),
        mk("GET", "/routing", "Every (defrouting) rule."),
        mk("GET", "/routing/resolve", "Resolved routing for `?host=…` (kind + target)."),
        mk("POST", "/outline", "Extract TOC from last-navigated page via (defoutline)."),
        mk("GET", "/annotate", "Every (defannotate) profile."),
        mk("GET", "/feeds", "Every (deffeed) subscription."),
        mk("GET", "/redirect", "Every (defredirect) rule."),
        mk("POST", "/redirect", "Rewrite a URL through LibRedirect-style rules."),
        mk("GET", "/url-clean", "Every (defurl-clean) rule."),
        mk("POST", "/url-clean", "Strip tracking params from a URL."),
        mk("GET", "/script-policy", "Every (defscript-policy) rule."),
        mk("GET", "/script-policy/resolve", "Resolved script policy for `?host=…`."),
        mk("GET", "/bridges", "Every (defbridge) entry."),
        mk("GET", "/bridges/torrc", "torrc `Bridge …` config block for every enabled bridge."),
        mk("GET", "/shares", "Every (defshare-target) destination."),
        mk("GET", "/offline", "Every (defoffline) save-for-later profile."),
        mk("GET", "/pull-to-refresh", "Every (defpull-to-refresh) rule."),
        mk("GET", "/pull-to-refresh/resolve", "Resolved PTR rule for `?host=…`."),
        mk("GET", "/downloads", "Every (defdownload) policy profile."),
        mk("GET", "/downloads/:name", "Full DownloadSpec for one policy."),
        mk("GET", "/autofill", "Every (defautofill) profile."),
        mk("GET", "/passwords", "Every (defpasswords) vault source (22 backends)."),
        mk("GET", "/passwords/for", "Vaults that auto-fill into `?host=…`."),
        mk("GET", "/auth-saver", "Every (defauth-saver) save-on-submit profile."),
        mk("GET", "/auth-saver/resolve", "Save profile for `?host=…`."),
        mk("GET", "/secure-notes", "Every (defsecure-note) non-password-secret profile."),
        mk("GET", "/passkeys", "Every (defpasskey) WebAuthn profile."),
        mk("GET", "/passkeys/for", "Passkey profiles permitting `?rp_id=…`."),
        mk("GET", "/llm-providers", "Every (defllm-provider) declaration."),
        mk("GET", "/summarize", "Every (defsummarize) profile."),
        mk("POST", "/summarize", "Run a summarize profile — body { profile, source }."),
        mk("GET", "/chat", "Every (defchat-with-page) profile."),
        mk("POST", "/chat", "Ask a chat profile — body { profile, question, page_context?, history? }."),
        mk("GET", "/llm-completion", "Every (defllm-completion) profile."),
        mk("POST", "/llm-completion", "Run a completion profile — body { profile, prefix }."),
        mk("GET", "/media-sessions", "Every (defmedia-session) profile."),
        mk("GET", "/media-sessions/resolve", "Media-session profile for `?host=…`."),
        mk("GET", "/casts", "Every (defcast) profile."),
        mk("GET", "/casts/applicable", "Cast profiles applicable to `?host=…`."),
        mk("GET", "/subtitles", "Every (defsubtitle) profile."),
        mk("GET", "/subtitles/resolve", "Subtitle profile for `?host=…`."),
        mk("GET", "/inspectors", "Every (definspector) panel."),
        mk("GET", "/inspectors/visible", "Only visible (definspector) panels."),
        mk("GET", "/inspectors/:name", "Full InspectorSpec for one panel."),
        mk("GET", "/profilers", "Every (defprofiler) profile."),
        mk("GET", "/profilers/:name", "Full ProfilerSpec for one profile."),
        mk("GET", "/console-rules", "Every (defconsole-rule)."),
        mk("GET", "/reader-aloud", "Every (defreader-aloud) profile."),
        mk("GET", "/reader-aloud/:name", "Full ReaderAloudSpec for one profile."),
        mk("GET", "/high-contrast", "Every (defhigh-contrast) profile."),
        mk("GET", "/high-contrast/resolve", "High-contrast profile for `?host=…`."),
        mk("GET", "/simplify", "Every (defsimplify) profile."),
        mk("GET", "/simplify/resolve", "Simplify profile for `?host=…`."),
        mk("GET", "/presence", "Every (defpresence) profile."),
        mk("GET", "/presence/resolve", "Presence profile for `?host=…`."),
        mk("GET", "/crdt-room", "Every (defcrdt-room) profile."),
        mk("GET", "/crdt-room/resolve", "CRDT-room profile for `?host=…`."),
        mk("GET", "/multiplayer-cursor", "Every (defmultiplayer-cursor) profile."),
        mk("GET", "/multiplayer-cursor/resolve", "Multiplayer-cursor profile for `?host=…`."),
        mk("GET", "/service-worker", "Every (defservice-worker) profile."),
        mk("GET", "/service-worker/resolve", "Service-worker profile for `?host=…`."),
        mk("GET", "/sync", "Every (defsync) channel."),
        mk("GET", "/sync/:name", "Full SyncSpec for one (defsync) channel by name."),
        mk("GET", "/sync/by-signal/:signal", "Every (defsync) channel syncing a given signal kind."),
        mk("GET", "/tab-group", "Every (deftab-group) profile."),
        mk("GET", "/tab-group/resolve", "Tab-group profile for `?host=…`."),
        mk("GET", "/tab-hibernate", "Every (deftab-hibernate) profile."),
        mk("GET", "/tab-hibernate/resolve", "Tab-hibernate profile for `?host=…`."),
        mk("GET", "/tab-preview", "Every (deftab-preview) profile."),
        mk("GET", "/tab-preview/resolve", "Tab-preview profile for `?host=…`."),
        mk("GET", "/search-engine", "Every (defsearch-engine) profile."),
        mk("GET", "/search-engine/default", "The current default search engine."),
        mk("GET", "/search-engine/:name", "Full SearchEngineSpec for one profile by name."),
        mk("GET", "/search-engine/by-keyword/:keyword", "Search engine for an omnibox keyword."),
        mk("GET", "/search-bang", "Every (defsearch-bang) shortcut."),
        mk("GET", "/search-bang/detect", "Detect a !bang in `?input=…`; returns {spec, remaining}."),
        mk("GET", "/identity", "Every (defidentity) persona."),
        mk("GET", "/identity/resolve", "Active identity for `?host=…`."),
        mk("GET", "/identity/:name", "Full IdentitySpec for one persona."),
        mk("GET", "/totp", "Every (deftotp) profile."),
        mk("GET", "/totp/:name", "Full TotpSpec for one profile."),
        mk("GET", "/totp/:name/code", "Current TOTP code + seconds-remaining."),
        mk("GET", "/totp/for-identity/:identity", "All TOTP profiles linked to a named identity."),
        mk("GET", "/reader", "Readability-style simplified view of the last navigated page (name=PROFILE selects)."),
        mk("GET", "/extensions", "Installed extension summary."),
        mk("POST", "/extensions", "Install an extension from raw Lisp source."),
        mk("GET", "/extensions/:name", "Full ExtensionSpec for one installed extension."),
        mk("DELETE", "/extensions/:name", "Uninstall an extension."),
        mk("POST", "/extensions/:name/enabled", "Toggle enabled state at runtime."),
        mk("GET", "/commands", "Every (defcommand) + the chords that bind to it."),
        mk("POST", "/commands/dispatch", "Simulate a typed key sequence; returns run/prefix/miss."),
        mk("GET", "/omnibox", "URL-bar autocomplete — ranks history+bookmarks+commands+search providers. `?q=…&profile=…`."),
        mk("POST", "/extensions/verify", "Verify a SignedExtension envelope against the trust DB."),
        mk("GET", "/trustdb", "List every trusted ed25519 pubkey (base64)."),
        mk("POST", "/trustdb", "Add a base64 ed25519 pubkey to the trust DB."),
        mk("DELETE", "/trustdb/:pubkey", "Revoke a trusted pubkey."),
    ]
}

fn mcp_tools() -> Vec<McpToolInfo> {
    let mk = |n: &str, d: &str| McpToolInfo {
        name: n.to_owned(),
        description: d.to_owned(),
    };
    vec![
        mk("status", "Namimado status — delegates to /status."),
        mk("version", "Crate version."),
        mk("navigate", "Navigate URL through the Lisp substrate."),
        mk("get_last_report", "Structured substrate report from most recent navigate."),
        mk("get_state", "Current state-store snapshot."),
        mk("get_dom_sexp", "Last navigated page absorbed into Lisp space."),
        mk("get_rules", "DSL rule inventory by keyword."),
        mk("reload", "Re-scan rc files + substrate.d/."),
        mk("new_tab", "(stub) open a new tab — requires running window."),
        mk("close_tab", "(stub) close a tab — requires running window."),
        mk("list_tabs", "(stub) list tabs — requires running window."),
        mk("get_bookmarks", "(stub) list bookmarks."),
        mk("add_bookmark", "(stub) bookmark a URL."),
        mk("config_get", "Read a config value by dotted key."),
        mk("config_set", "(runtime-only) set a config value."),
        mk("get_accessibility_tree", "ARIA AX tree from the last navigate — canonical n-* → role."),
        mk("history_recent", "Most recent browsing history entries — auto-recorded on every navigate."),
        mk("history_search", "Search history by title/URL substring."),
        mk("storage_list", "Per-store summary of every (defstorage …) declared store."),
        mk("storage_entries", "Full key→value snapshot of one store."),
        mk("storage_get", "Read one key from a store."),
        mk("storage_set", "Write one key→value into a store."),
        mk("storage_delete", "Delete one key from a store."),
        mk("storage_index_list", "Declared secondary indexes + distinct values."),
        mk("storage_by_index", "O(log n) entry lookup by indexed projection."),
        mk("storage_by_index_range", "Inclusive range scan over a secondary index."),
        mk("i18n_get", "Resolve one translated message with fallback chain."),
        mk("i18n_coverage", "Translation coverage — locales + missing keys."),
        mk("security_policy", "Per-host security-policy headers (CSP + Permissions-Policy + more)."),
        mk("find", "Find-in-page against the last navigated document."),
        mk("zoom", "Resolved zoom + text-only flag for a host."),
        mk("snapshot_recipe", "Declarative snapshot recipe for a host."),
        mk("pip", "Picture-in-picture rule for a host."),
        mk("gesture_dispatch", "Dispatch a mouse-gesture stroke → command."),
        mk("boosts_list", "List (defboost) overlays, optionally by host."),
        mk("boost_set_enabled", "Toggle a boost at runtime."),
        mk("session_open", "Currently-open session tabs."),
        mk("session_closed", "Recently-closed session tabs."),
        mk("session_undo_close", "Pop the most-recently-closed tab."),
        mk("js_eval", "Evaluate JS-ish source through the active JsRuntime."),
        mk("spaces_list", "List all (defspace) declarations."),
        mk("space_get", "Full SpaceSpec for one space."),
        mk("space_activate", "Activate a space."),
        mk("space_active", "Currently active space."),
        mk("sidebars_list", "List (defsidebar) apps, optionally by host."),
        mk("splits_list", "List all (defsplit) layouts."),
        mk("split_get", "Full SplitSpec for one layout."),
        mk("spoofs_list", "List all (defspoof) profiles."),
        mk("spoof_for", "Resolve (defspoof) for a host."),
        mk("dns_list", "List all (defdns) resolver profiles."),
        mk("dns_get", "Full DnsSpec for one resolver."),
        mk("routing_list", "List all (defrouting) rules."),
        mk("routing_resolve", "Resolved route for a host — kind + target."),
        mk("outline_extract", "Extract TOC from last-navigated page."),
        mk("annotate_list", "All (defannotate) profiles."),
        mk("feed_list", "All (deffeed) subscriptions."),
        mk("redirect_list", "All (defredirect) rules."),
        mk("redirect_apply", "Rewrite a URL through LibRedirect-style rules."),
        mk("url_clean_list", "All (defurl-clean) rules."),
        mk("url_clean_apply", "Strip tracking params from a URL."),
        mk("script_policy_list", "All (defscript-policy) rules."),
        mk("script_policy_for", "Resolved script policy for a host."),
        mk("bridge_list", "All (defbridge) entries."),
        mk("bridges_torrc_block", "torrc block for enabled bridges."),
        mk("share_list", "Every (defshare-target) destination."),
        mk("offline_list", "Every (defoffline) profile."),
        mk("pull_refresh_list", "Every (defpull-to-refresh) rule."),
        mk("pull_refresh_for", "Resolved PTR rule for a host."),
        mk("download_list", "Every (defdownload) policy."),
        mk("download_get", "Full DownloadSpec for one policy."),
        mk("autofill_list", "Every (defautofill) profile."),
        mk("password_list", "Every (defpasswords) vault source."),
        mk("passwords_for", "Vaults that auto-fill into a host."),
        mk("auth_saver_list", "Every (defauth-saver) profile."),
        mk("auth_saver_for", "Save-on-submit profile for a host."),
        mk("secure_note_list", "Every (defsecure-note) profile."),
        mk("passkey_list", "Every (defpasskey) WebAuthn profile."),
        mk("passkeys_for", "Passkey profiles permitting an RP ID."),
        mk("llm_provider_list", "Every (defllm-provider)."),
        mk("summarize_list", "Every (defsummarize) profile."),
        mk("summarize_run", "Run a summarize profile."),
        mk("chat_list", "Every (defchat-with-page) profile."),
        mk("chat_ask", "Ask a chat profile against page context + history."),
        mk("llm_completion_list", "Every (defllm-completion) profile."),
        mk("llm_completion_run", "Run a completion profile against a prefix."),
        mk("media_session_list", "Every (defmedia-session) profile."),
        mk("media_session_for", "Media-session profile for a host."),
        mk("cast_list", "Every (defcast) profile."),
        mk("cast_applicable", "Cast profiles applicable to a host."),
        mk("subtitle_list", "Every (defsubtitle) profile."),
        mk("subtitle_for", "Subtitle profile for a host."),
        mk("inspector_list", "Every (definspector) panel."),
        mk("inspector_visible", "Only visible inspector panels."),
        mk("inspector_get", "Full InspectorSpec for one panel."),
        mk("profiler_list", "Every (defprofiler) profile."),
        mk("profiler_get", "Full ProfilerSpec for one profile."),
        mk("console_rule_list", "Every (defconsole-rule)."),
        mk("reader_aloud_list", "Every (defreader-aloud) profile."),
        mk("reader_aloud_get", "Full ReaderAloudSpec for one profile."),
        mk("high_contrast_list", "Every (defhigh-contrast) profile."),
        mk("high_contrast_for", "High-contrast profile for a host."),
        mk("simplify_list", "Every (defsimplify) profile."),
        mk("simplify_for", "Simplify profile for a host."),
        mk("presence_list", "Every (defpresence) profile."),
        mk("presence_for", "Presence profile for a host."),
        mk("crdt_room_list", "Every (defcrdt-room) profile."),
        mk("crdt_room_for", "CRDT-room profile for a host."),
        mk("multiplayer_cursor_list", "Every (defmultiplayer-cursor) profile."),
        mk("multiplayer_cursor_for", "Multiplayer-cursor profile for a host."),
        mk("service_worker_list", "Every (defservice-worker) profile."),
        mk("service_worker_for", "Service-worker profile for a host."),
        mk("sync_list", "Every (defsync) channel."),
        mk("sync_get", "Full SyncSpec for one (defsync) channel by name."),
        mk("sync_for_signal", "Every (defsync) channel syncing a given signal kind."),
        mk("tab_group_list", "Every (deftab-group) profile."),
        mk("tab_group_for", "Tab-group profile for a host."),
        mk("tab_hibernate_list", "Every (deftab-hibernate) profile."),
        mk("tab_hibernate_for", "Tab-hibernate profile for a host."),
        mk("tab_preview_list", "Every (deftab-preview) profile."),
        mk("tab_preview_for", "Tab-preview profile for a host."),
        mk("search_engine_list", "Every (defsearch-engine) profile."),
        mk("search_engine_get", "Full SearchEngineSpec for one profile by name."),
        mk("search_engine_by_keyword", "Search engine for an omnibox keyword shortcut."),
        mk("search_engine_default", "The current default search engine."),
        mk("search_bang_list", "Every (defsearch-bang) shortcut."),
        mk("search_bang_detect", "Detect a !bang in input; returns {spec, remaining}."),
        mk("identity_list", "Every (defidentity) persona."),
        mk("identity_get", "Full IdentitySpec for one persona by name."),
        mk("identity_for", "Active identity for a host."),
        mk("totp_list", "Every (deftotp) profile."),
        mk("totp_get", "Full TotpSpec for one profile by name."),
        mk("totp_for_identity", "All TOTP profiles linked to a named identity."),
        mk("totp_code", "Current TOTP code + seconds-remaining for a profile."),
        mk("reader", "Readability-style simplified view of the last navigated page."),
        mk("extensions_list", "Installed extension summary."),
        mk("extension_get", "Full ExtensionSpec for one extension."),
        mk("extension_install", "Install (defextension) bundle from raw Lisp source."),
        mk("extension_set_enabled", "Toggle extension enabled state at runtime."),
        mk("extension_remove", "Uninstall an extension."),
        mk("commands_list", "Every (defcommand) + its bound chords."),
        mk("dispatch_key", "Simulate a typed key sequence against (defbind)s."),
        mk("omnibox", "URL-bar autocomplete — history+bookmarks+commands+search+navigate."),
        mk("verify_extension", "Verify a SignedExtension against the trust DB."),
        mk("trustdb_list", "List every trusted ed25519 pubkey."),
        mk("trustdb_add", "Add a pubkey to the trust DB."),
        mk("trustdb_revoke", "Revoke a trusted pubkey."),
    ]
}

fn features() -> Vec<String> {
    let mut out = Vec::new();
    if cfg!(feature = "browser-core") {
        out.push("browser-core".to_owned());
    }
    if cfg!(feature = "gpu-chrome") {
        out.push("gpu-chrome".to_owned());
    }
    if cfg!(feature = "http-server") {
        out.push("http-server".to_owned());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typescape_contains_all_shipped_packs() {
        let ts = typescape();
        let files: Vec<_> = ts.normalize_packs.iter().map(|p| p.file.as_str()).collect();
        for expected in PACK_FILES {
            assert!(files.contains(expected), "missing pack file: {expected}");
        }
    }

    #[test]
    fn typescape_hash_is_128_bit_base32() {
        let ts = typescape();
        assert_eq!(ts.hash.len(), 26);
        for ch in ts.hash.chars() {
            assert!(ch.is_ascii_lowercase() || ch.is_ascii_digit());
        }
    }

    #[test]
    fn typescape_is_deterministic() {
        let a = typescape();
        let b = typescape();
        assert_eq!(a.hash, b.hash);
    }

    #[test]
    fn pack_kind_matches_filename_convention() {
        // Inbound packs do NOT end in -emit.lisp and aren't blocker-*.
        // Emit packs end in -emit.lisp. Blocker packs start with blocker-*.
        let ts = typescape();
        for pack in &ts.normalize_packs {
            let is_emit = pack.file.ends_with("-emit.lisp");
            let is_blocker = pack.file.starts_with("blocker-");
            let is_keybindings = pack.file.ends_with("-mode.lisp");
            match pack.kind {
                PackKind::Emit => assert!(is_emit, "pack {} marked Emit but not -emit.lisp", pack.file),
                PackKind::Blocker => assert!(is_blocker, "pack {} marked Blocker but not blocker-*", pack.file),
                PackKind::Keybindings => assert!(
                    is_keybindings,
                    "pack {} marked Keybindings but not *-mode.lisp",
                    pack.file
                ),
                PackKind::Inbound => {
                    assert!(!is_emit, "pack {} marked Inbound but ends in -emit.lisp", pack.file);
                    assert!(!is_blocker, "pack {} marked Inbound but starts with blocker-", pack.file);
                    assert!(!is_keybindings, "pack {} marked Inbound but ends in -mode.lisp", pack.file);
                }
            }
        }
    }

    #[test]
    fn every_http_endpoint_has_method_path_description() {
        let ts = typescape();
        assert!(!ts.http_endpoints.is_empty());
        for ep in &ts.http_endpoints {
            assert!(!ep.method.is_empty());
            assert!(ep.path.starts_with('/'));
            assert!(!ep.description.is_empty());
        }
    }

    #[test]
    fn mcp_tool_names_are_unique() {
        let ts = typescape();
        let mut seen = std::collections::HashSet::new();
        for t in &ts.mcp_tools {
            assert!(seen.insert(t.name.clone()), "duplicate MCP tool: {}", t.name);
        }
    }

    #[test]
    fn http_endpoint_paths_are_unique() {
        let ts = typescape();
        let mut seen = std::collections::HashSet::new();
        for ep in &ts.http_endpoints {
            let key = format!("{} {}", ep.method, ep.path);
            assert!(seen.insert(key.clone()), "duplicate HTTP endpoint: {key}");
        }
    }

    #[cfg(feature = "browser-core")]
    #[test]
    fn typescape_embeds_nami_core_leaf() {
        let ts = typescape();
        let keywords = ts.nami_core
            .get("dsl_keywords")
            .and_then(|v| v.as_array())
            .expect("dsl_keywords array present");
        assert_eq!(keywords.len(), 78, "78 DSL keywords expected in nami-core");
    }
}
