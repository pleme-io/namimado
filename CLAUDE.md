# Namimado (波窓) — Desktop Web Browser

A fully programmable desktop web browser. Servo handles web content rendering.
garasu/egaku/irodzuki render the browser chrome (tabs, address bar, sidebar,
status bar) as GPU-native widgets. nami-core provides shared browser
infrastructure (bookmarks, history, content blocking). Rhai scripting and
an embedded MCP server make it automation-first.

Crate/binary name: `namimado`.

## Build & Test

```bash
cargo build                          # default features (scaffold only)
cargo build --features gpu-chrome    # with garasu/egaku chrome
cargo build --features full          # browser-core + gpu-chrome
cargo test
nix build                            # full Nix package
nix run .#rebuild                    # rebuild HM module (from nix repo)
```

## Competitive Position

| vs | Namimado advantage |
|----|-------------------|
| **Firefox/Chrome** | Fully programmable via Rhai, MCP-drivable, Nix-configured, zero telemetry, GPU-native chrome |
| **Nyxt** | Rust not Common Lisp, GPU chrome via garasu (not GTK), Servo engine, pleme-io library ecosystem |
| **qutebrowser** | Native GPU rendering (not Qt WebEngine wrapper), embedded MCP server, Rhai plugins |
| **Servo (standalone)** | Namimado adds the application layer: tabs, bookmarks, history, scripting, MCP, Nix integration |
| **Ladybird** | Rust not C++, Servo is further along, Rhai scripting, MCP, Nix-native build |

## Architecture

### Layer Separation

```
+-------------------------------------------------------------------+
|                      namimado (this repo)                         |
|                                                                   |
|  +-------------+  +------------------+  +-----------------------+ |
|  | app.rs      |  | browser/         |  | chrome/               | |
|  | CLI, window |  | tab, tabs,       |  | toolbar, sidebar,     | |
|  | event loop  |  | navigation       |  | statusbar             | |
|  +------+------+  +--------+---------+  +----------+------------+ |
|         |                  |                        |              |
|  +------+------------------+------------------------+------------+ |
|  |                    config.rs                                  | |
|  |  NamimadoConfig (shikumi-loaded, typed, hot-reloadable)       | |
|  +---------------------------------------------------------------+ |
|         |                  |                        |              |
|  +------+------+  +-------+--------+  +------------+------------+ |
|  | webview/    |  | ipc/           |  | (future)                | |
|  | engine.rs   |  | bridge.rs      |  | mcp.rs, scripting.rs    | |
|  | Servo embed |  | Rust <-> JS    |  | kaname, soushi          | |
|  +------+------+  +-------+--------+  +-------------------------+ |
+-------------------------------------------------------------------+
          |                  |
+---------+------------------+---------------------------+
|                External dependencies                    |
|  Servo (web engine)    garasu (GPU chrome)              |
|  nami-core (browser)   egaku (chrome widgets)           |
|  winit (windowing)     irodzuki (GPU theming)           |
|  shikumi (config)      kaname (MCP)                     |
|  soushi (Rhai)         awase (hotkeys)                   |
+---------------------------------------------------------+
```

### Current State

The repo is in **scaffold phase**. The architecture is defined, module structure
is laid out, and basic tab management + navigation + config + IPC bridge work.
The Servo engine integration is stubbed (`WebViewEngine` is a scaffold that
logs operations but does not render web content yet).

### Source Modules

| Module | Purpose | Status |
|--------|---------|--------|
| `main.rs` | CLI (clap), tracing init, delegates to `app::run()` | Done |
| `app.rs` | `App` struct, winit `ApplicationHandler`, window creation, IPC dispatch | Done |
| `browser/tab.rs` | `Tab` state (URL, title, loading, history back/forward stacks), `TabId` | Done, tested |
| `browser/tabs.rs` | `TabManager` (add/close/switch/reorder tabs, active tracking) | Done, tested |
| `browser/navigation.rs` | `normalize_url()` (URL/domain/search), `navigate()`, `go_back/forward()`, `reload/stop()` | Done, tested |
| `chrome/toolbar.rs` | Toolbar widget (address bar, nav buttons, reload) | Scaffold |
| `chrome/sidebar.rs` | Sidebar widget (bookmarks, history, devtools panels) | Scaffold |
| `chrome/statusbar.rs` | Status bar (hovered link URL, loading progress, security indicator) | Scaffold |
| `config.rs` | `NamimadoConfig` with `ThemeConfig`, `ContentBlockingConfig`, `PrivacyConfig` | Done, tested |
| `webview/engine.rs` | `WebViewEngine` (Servo/wry wrapper: navigate, evaluate_js, IPC) | Scaffold |
| `ipc/bridge.rs` | `IpcBridge` + `IpcMessage` (Navigate, TitleChanged, LoadStart/End, FaviconChanged) | Done, tested |
| `module/default.nix` | Home-manager module | Done |

### Key Design: Chrome vs Content Split

The browser window is split into two rendering domains:

1. **Browser chrome** (garasu/egaku) -- Toolbar, tab bar, sidebar, status bar.
   These are GPU-rendered widgets managed entirely by Rust. They compose
   on top of the content area. The theme comes from irodzuki (base16 to
   wgpu uniforms).

2. **Web content** (Servo) -- The actual web page. Servo owns its own
   rendering pipeline. Namimado embeds Servo and composites its output
   into the window alongside the GPU chrome.

The `IpcBridge` coordinates between these domains: Servo sends page events
(title change, load complete, favicon) to Rust, and Rust sends navigation
commands to Servo.

---

## Shared Library Integration

| Library | Used For |
|---------|----------|
| **nami-core** | Bookmarks, history, content blocking (shared with aranami) |
| **garasu** | GPU rendering for browser chrome (tabs, toolbar, sidebar) |
| **egaku** | Chrome widget state machines (address bar input, tab bar, bookmark list) |
| **irodzuki** | GPU theming (base16 to wgpu uniforms, dark/light mode) |
| **shikumi** | Config discovery + hot-reload (`~/.config/namimado/namimado.yaml`) |
| **kaname** | Embedded MCP server (stdio transport) |
| **soushi** | Rhai scripting engine for user plugins |
| **awase** | Keyboard shortcuts (modal vim-style + browser standard) |
| **hasami** | Clipboard (copy URL, page text) |
| **tsunagu** | Daemon IPC (for headless/background browsing mode) |
| **tsuuchi** | Desktop notifications (download complete, permission requests) |
| **todoku** | HTTP client for extension downloads, update checks |
| **mojiban** | Rich text in sidebar panels (reader mode, devtools) |

### Feature gates

| Feature | Enables | Dependencies |
|---------|---------|--------------|
| `default` | Minimal scaffold (winit, clap, serde) | None optional |
| `browser-core` | nami-core integration (bookmarks, history, blocking) | `nami-core` |
| `gpu-chrome` | GPU-rendered browser chrome | `garasu`, `egaku`, `irodzuki`, `shikumi` |
| `full` | Everything | `browser-core` + `gpu-chrome` |

---

## Configuration

- **File**: `~/.config/namimado/namimado.yaml`
- **Env override**: `NAMIMADO_CONFIG=/path/to/config.yaml`
- **Env prefix**: `NAMIMADO_` (e.g., `NAMIMADO_HOMEPAGE=https://example.com`)
- **Hot-reload**: shikumi ArcSwap + file watcher (requires `gpu-chrome` feature)
- **HM module**: `blackmatter.components.namimado.*`

Config structure (currently implemented):
```yaml
homepage: "about:blank"
search_engine: "https://www.google.com/search?q=%s"
devtools_enabled: false
theme:
  dark: true
  font_size: 14.0
  toolbar_opacity: 1.0
content_blocking:
  block_third_party_cookies: true
  block_trackers: true
  block_ads: false
privacy:
  clear_on_exit: false
  do_not_track: true
  https_only: false
```

Target additions:
```yaml
keybindings: {}               # override default keybindings
sidebar:
  visible: true
  position: "left"            # or "right"
  width: 300
downloads:
  directory: "~/Downloads"
  ask_location: false
permissions:
  geolocation: "ask"          # "allow", "deny", "ask"
  notifications: "ask"
  camera: "deny"
  microphone: "deny"
```

---

## MCP Server (kaname)

Embedded MCP server via stdio transport, discoverable at `~/.config/namimado/mcp.json`.

**Standard tools**: `status`, `config_get`, `config_set`, `version`

**Browser-specific tools**:
| Tool | Description |
|------|-------------|
| `navigate` | Navigate active tab to a URL |
| `get_url` | Get current URL of active tab |
| `get_title` | Get current page title |
| `get_dom` | Get DOM tree or subtree (via CSS selector) as JSON |
| `evaluate_js` | Execute JavaScript in page context |
| `screenshot` | Capture viewport or full page as PNG |
| `tab_list` | List all open tabs with URLs and titles |
| `tab_new` | Open a new tab (optionally with URL) |
| `tab_close` | Close a tab by index or ID |
| `tab_switch` | Switch to a tab by index or ID |
| `bookmark_add` | Add current page to bookmarks with tags |
| `bookmark_list` | List bookmarks (optional search query) |
| `history_search` | Search browsing history |
| `devtools_open` | Open developer tools panel |
| `network_requests` | Get recent network requests for current page |
| `content_block_stats` | Get content blocking statistics |

---

## Plugin System (soushi + Rhai)

Scripts loaded from `~/.config/namimado/scripts/*.rhai`.

**Rhai API**:
```
browser.goto(url)            // navigate active tab
browser.tab_new()            // open new tab
browser.tab_close()          // close active tab
browser.tab_switch(n)        // switch to tab n
browser.js(script)           // evaluate JavaScript in page
browser.dom_query(selector)  // query DOM elements
browser.back()               // go back in history
browser.forward()            // go forward in history
browser.reload()             // reload current page
browser.bookmark(url)        // bookmark URL
browser.screenshot(path)     // save screenshot to path
browser.url()                // get current URL
browser.title()              // get current page title
browser.sidebar_toggle()     // toggle sidebar visibility
browser.devtools_toggle()    // toggle devtools panel
browser.find(text)           // find text on page
```

**Event hooks**: `on_page_load`, `on_navigate`, `on_tab_open`, `on_tab_close`,
`on_download_start`, `on_download_complete`, `on_permission_request`,
`on_blocked_request`, `on_error`

**Use cases**:
- Custom new tab page with frequently visited sites
- Auto-reader-mode for specific domains
- Privacy automations (clear cookies on domain exit)
- Page content extraction and transformation
- Automated testing workflows
- Custom keyboard macros

---

## Hotkey System (awase)

Hybrid navigation: browser-standard shortcuts + vim-style modal bindings.

**Browser-standard shortcuts** (always active):
| Key | Action |
|-----|--------|
| `Cmd+T` | New tab |
| `Cmd+W` | Close tab |
| `Cmd+L` | Focus address bar |
| `Cmd+R` | Reload |
| `Cmd+[` / `Cmd+]` | Back / Forward |
| `Cmd+1`..`Cmd+9` | Switch to tab N |
| `Cmd+Shift+T` | Reopen last closed tab |
| `Cmd+F` | Find on page |
| `Cmd+D` | Bookmark current page |
| `Cmd+Shift+B` | Toggle bookmarks sidebar |

**Vim-style bindings** (togglable, `Cmd+Shift+V` to enable):
| Mode | Purpose | Enter via |
|------|---------|-----------|
| **Normal** | Page navigation | `Esc` |
| **Insert** | Text input (forms, address bar) | `i`, click in input |
| **Command** | `:` prefix commands | `:` |
| **Follow** | Link hint labels | `f` |

Normal mode follows the same bindings as aranami (`j/k` scroll, `f` follow,
`o` open URL, `H/L` back/forward, etc.) for consistency.

---

## Servo Integration

### Current state

The `WebViewEngine` is a scaffold. It stores the current URL and has method
signatures for `navigate()`, `evaluate_js()`, and `inject_ipc_bridge()`, but
no actual web engine is linked.

### Integration plan

Servo does not yet publish a clean embedding crate. The integration strategy:

1. **Build Servo from source** via Nix (substrate build helper). The Nix build
   fetches the Servo source tree, applies patches for embedding mode, and
   produces a library that namimado links against.

2. **WebViewEngine** wraps the Servo embedding API:
   - Create a Servo instance with the winit window handle
   - Forward navigation commands (URL changes, back/forward, reload)
   - Receive page events via Servo callbacks (title change, load status)
   - Composite Servo's rendered output into the garasu render pass

3. **Content area layout**: The garasu chrome renders toolbar/sidebar/statusbar.
   The remaining rectangle is the content area, passed to Servo as the viewport.
   Servo renders into its own surface/texture, which garasu composites.

4. **Fallback**: If Servo integration proves too complex initially, use
   `wry` (Tauri's WebView wrapper) as an intermediate step. wry uses the
   platform WebView (WebKit on macOS, WebKitGTK on Linux) and is much simpler
   to embed, but sacrifices cross-platform rendering consistency.

---

## Roadmap

### Phase 1 -- Application Shell [DONE]
winit window, tab model, navigation logic, URL normalization, IPC bridge,
config with shikumi, HM module.

### Phase 2 -- GPU Chrome [NEXT]
Wire garasu + egaku for browser chrome rendering. Toolbar with address bar.
Tab bar with tab switching. Status bar. Sidebar scaffold.

### Phase 3 -- Servo Integration
Build Servo via Nix. Embed Servo in the content area. Wire navigation and
page events through IPC bridge. Composite Servo output with GPU chrome.

### Phase 4 -- Browser Core
Wire nami-core for bookmarks, history, content blocking. Sidebar panels
for bookmarks/history. Download manager. Cookie management.

### Phase 5 -- Programmability
MCP server (kaname). Rhai scripting (soushi). Plugin loading from
`~/.config/namimado/scripts/`. Extension compatibility layer.

### Phase 6 -- Advanced Features
DevTools panel (via Servo's DevTools integration). Reader mode (mojiban).
PDF viewer. Print support. Multi-window. Session restore.

### Phase 7 -- Polish
Performance profiling. Memory optimization. Accessibility (screen reader,
keyboard-only navigation, high contrast). Platform-specific polish
(macOS native menu bar, Linux desktop entry, Wayland support).

---

## Design Decisions

### Why Servo (not Chromium/WebKit)?
Servo is the only major browser engine written in Rust. It aligns with the
all-Rust, zero-C-dependency philosophy of the pleme-io ecosystem. Servo's
parallel layout and rendering architecture is also forward-looking. The
tradeoff is less web compatibility today vs. Chromium, but Servo is rapidly
improving and Mozilla/Linux Foundation back it.

### Why GPU chrome (not Servo for everything)?
Using garasu/egaku for browser chrome gives us full control over the toolbar,
tabs, sidebar, and status bar. We can theme them with irodzuki (base16 colors),
make them fully scriptable via Rhai, and keep them fast regardless of web
content load. Servo renders only the web content area.

### Why winit (not madori)?
Namimado uses winit directly (via `ApplicationHandler`) because it needs
fine-grained control over window lifecycle, resize handling, and compositing
between the GPU chrome and Servo content area. Madori's `App::builder()`
pattern is designed for single-surface apps where one `RenderCallback` owns
the entire frame. Namimado's dual-surface architecture (garasu chrome + Servo
content) requires more control than madori provides.

### Why nami-core (not inline)?
Bookmarks, history, and content blocking are shared with aranami. Keeping
them in nami-core ensures both browsers have identical behavior and avoids
code duplication.

### Why feature gates?
The `browser-core` and `gpu-chrome` features allow building the scaffold
without all dependencies present. This is useful during early development
when Servo/nami-core may not be ready. The `full` feature enables everything
for production builds.

---

## Nix Integration

- **Flake**: Uses substrate `rust-tool-release-flake.nix` for multi-platform builds
- **HM module path**: `module/default.nix` using substrate `hm-service-helpers.nix`
- **Servo build**: Will use a custom Nix derivation (similar to blackmatter-ghostty's impure build pattern) since Servo has complex build dependencies
- **Config management**: HM module generates `~/.config/namimado/namimado.yaml` from typed Nix options
