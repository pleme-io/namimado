# Browser landscape — capabilities and absorption plan

How namimado stacks up against popular browsers, and the concrete
path to absorbing each missing capability **within our pattern**:
typed Rust core, Lisp substrate, WASM sandbox, typescape-described,
BLAKE3-attested.

## The rule

Every capability we absorb lands as:

1. **A Rust type** expressing the invariants (immutable where possible).
2. **A `(def*)` DSL** — Lisp authors it declaratively, TataraDomain
   derives the compile pass, typescape lists it.
3. **A trait-based extension point** so substitutes can ship without
   forking — e.g. any `Fetcher` for networking, any
   `CompositeRenderer` for painting.
4. **Tests** — unit + proptest where the contract is abstract.
5. **Provenance** — every effect stamps an attr so the chain is
   inspectable.

No ambient JS. No unchecked native calls. No capability that isn't
typed. Capabilities compose the way pleme-io's modules compose:
lattice joins, not runtime flags.

## Capability matrix

| Capability                    | Chromium | Firefox | Safari | Servo | Ladybird | Nyxt  | qutebr | namimado  |
| ----------------------------- | -------- | ------- | ------ | ----- | -------- | ----- | ------ | --------- |
| **HTML5 parser**              | ✅       | ✅      | ✅     | ✅    | ✅       | ✅    | ✅     | ✅ html5ever |
| **CSS3 layout + render**      | ✅       | ✅      | ✅     | ✅    | 🟡       | ✅    | ✅     | 🟡 layout only (taffy) |
| **JS engine**                 | V8       | SpMonkey| JSC    | SpMonkey| LibJS  | ECL   | QtWebEngine | ❌ (see J1) |
| **WASM runtime**              | ✅       | ✅      | ✅     | ✅    | 🟡       | ❌    | ✅     | ✅ wasmtime (ours is capability-gated) |
| **HTTP/2 + HTTP/3 fetch**     | ✅       | ✅      | ✅     | 🟡    | 🟡       | ✅    | ✅     | 🟡 reqwest blocking (HTTP/2 ok, no QUIC) |
| **TLS / HSTS / cookies**      | ✅       | ✅      | ✅     | ✅    | 🟡       | ✅    | ✅     | 🟡 rustls via reqwest; no cookie jar yet |
| **Service Workers / PWA**     | ✅       | ✅      | ✅     | ❌    | ❌       | ❌    | ❌     | ❌ (see J2) |
| **Tabs + sessions**           | ✅       | ✅      | ✅     | ❌    | 🟡       | ✅    | ✅     | 🟡 scaffolded, not wired |
| **History / bookmarks**       | ✅       | ✅      | ✅     | ❌    | ❌       | ✅    | ✅     | 🟡 scaffolded, not wired |
| **Download manager**          | ✅       | ✅      | ✅     | ❌    | ❌       | ✅    | ✅     | 🟡 scaffolded |
| **Extensions (WebExtensions)**| ✅       | ✅      | ✅     | ❌    | ❌       | ✅(Lisp)| 🟡    | ✅ via `(defwasm-agent)` + Lisp rule packs |
| **DevTools / inspector**      | ✅       | ✅      | ✅     | 🟡    | 🟡       | ✅    | ✅     | ✅ `/ui` panel, `/typescape`, live reload |
| **CDP / WebDriver automation**| ✅       | ✅      | ✅     | ❌    | ❌       | ❌    | ❌     | ✅ HTTP+MCP (richer than CDP); compat shim queued |
| **Reader mode / text extract**| ✅       | ✅      | ✅     | ❌    | ❌       | ✅    | 🟡     | ✅ `(defplan :name "reader-mode")` + text_render |
| **Content blocking**          | 🟡       | ✅ uBO  | 🟡     | ❌    | ❌       | ✅    | ✅     | 🟡 primitive list; absorption below |
| **Framework awareness**       | ❌       | ❌      | ❌     | ❌    | ❌       | ❌    | ❌     | ✅ 20 frameworks detected, normalize packs |
| **User-scripts / Greasemonkey**| 🟡 ext  | ✅ ext  | ❌     | ❌    | ❌       | ✅ Lisp| ✅ Python| ✅ 14 def* DSLs + `<l-eval>` |
| **Headless mode**             | ✅       | ✅      | 🟡     | 🟡    | ❌       | ❌    | ✅     | ✅ `serve` / `navigate` / `mcp` |
| **Typed, attestable surface** | ❌       | ❌      | ❌     | ❌    | ❌       | ❌    | ❌     | ✅ typescape + BLAKE3 |
| **MCP / agent-native**        | ❌       | ❌      | ❌     | ❌    | ❌       | ❌    | ❌     | ✅ 16 MCP tools, first-class |

**Legend:** ✅ full · 🟡 partial · ❌ absent.

### Our only-in-namimado columns

Framework awareness (detection + normalize to canonical `n-*`),
typed attestable surface (typescape + BLAKE3), MCP/agent-native
controls, user-scripts as Lisp DSLs. Nobody else does these.

### Our biggest gaps

JS engine, service workers, CSS painting, cookie jar, tab wiring,
CDP compat, extension marketplace.

---

## Absorption plans

Each plan is a single-session arc unless marked multi-session.
Every one lists the new DSL / Rust type / trait / host-function
additions and the typescape entry that makes it discoverable.

### J1. JS runtime — absorbed as a capability-gated agent

**The mainstream way**: every page gets ambient JS; permissions
retrofitted later (CSP, sandboxing, iframe origins).

**Our way**: JS is a WASM agent, not ambient. A page declares

```lisp
(defjs-agent :name "ticker"
             :src "ticker.js"
             :when "has-selector? \"ticker-widget\""
             :caps (dom-query dom-emit))
```

The JS engine (`boa_engine` for pure Rust, or QuickJS-WASI for
sandboxing) runs as a WASM guest via our existing host. Host
functions `nami.query_count` / `dom_sexp_read` / `emit` are
already there. No page gets JS by default; capability grants
are explicit.

**New types**: `JsAgentSpec`, `JsRuntime` enum `{ Boa, QuickJs }`.
**Scope**: 2 sessions (engine integration + DSL).
**Leverages**: `WasmHost`, `(defwasm-agent)` machinery, typescape.

### J2. Service Workers — absorbed as persistent WASM

**The mainstream way**: a JS thread lives past navigation, intercepts
fetches, caches, serves offline.

**Our way**: long-lived WASM agent registered with the `fetch`
capability.

```lisp
(defservice-worker :name "offline-cache"
                   :wasm "cache-worker.wasm"
                   :scope "/"
                   :caps (fetch cache-rw))
```

The worker module exports `fetch_handler`; our request pipeline
calls into it before falling through to reqwest. Cache is a
capability-gated store backed by `~/.cache/namimado/<worker-id>/`.

**New types**: `ServiceWorkerSpec`, `FetchCapability`.
**Scope**: 1 session after J1.

### J3. Cookie jar + storage — `(defstorage)`

**The mainstream way**: implicit per-origin cookie jar; localStorage;
IndexedDB.

**Our way**: declarative storage backends, SQLite-backed, per-agent
capability.

```lisp
(defstorage :name "sessions"
            :backend :sqlite
            :path "~/.local/share/namimado/sessions.db"
            :caps (read write))

(defstorage :name "cookie-jar"
            :backend :sqlite
            :scope :per-origin
            :ttl 30d)
```

**New types**: `StorageSpec`, `StorageBackend` enum.
**Scope**: 1 session.

### J4. Tabs / history / bookmarks — promoted to substrate state

**The mainstream way**: three separate subsystems.

**Our way**: one DSL per, all backed by state cells + a SQLite
storage pack, all authorable as Lisp.

```lisp
(deftab          :kind (builtin open close switch reorder))
(defhistory      :max-entries 10000 :ttl 90d)
(defbookmark     :folder "/" :pinned #f)

;; ... and a rule that ties them together:
(defagent :on "tab-closed" :when "history-deep" :apply "bookmark-page")
```

**New types**: `Tab`, `HistoryEntry`, `Bookmark`. **These types
already exist** in namimado as scaffolding — this arc wires them
into the substrate store.

**Scope**: 1 session.

### J5. Content blocking — `(defblocker)` + uBlock rule pack

**The mainstream way**: uBlock Origin, ABP — extension shipping
rule lists interpreted by JS.

**Our way**: a `(defblocker)` DSL absorbs EasyList / EasyPrivacy
syntax into normalize rules. Matching requests rewrite to
`about:blocked`; matching DOM subtrees rewrite to
`<n-blocked data-n-from=…>`.

```lisp
(defblocker :name "easylist"
            :list "~/.config/namimado/blocklists/easylist.txt"
            :scope network+dom)

(defblocker :name "tracker-domains"
            :domains ("google-analytics.com" "doubleclick.net")
            :scope network)
```

**New types**: `BlockerSpec`, `BlockOutcome`, `BlockScope` enum.
**Leverages**: existing `nami_core::content` module, normalize
engine.
**Scope**: 1 session (authoring) + ongoing packs.

### J6. CDP / WebDriver compat — `namimado cdp-proxy`

**The mainstream way**: Chrome DevTools Protocol + WebDriver each
speak JSON-over-WS or JSON-over-HTTP.

**Our way**: a thin translation layer in namimado that speaks CDP
and dispatches to our own HTTP/MCP surface. One-to-one mapping:

| CDP                      | Ours                       |
| ------------------------ | -------------------------- |
| `Target.createTarget`    | `POST /navigate`           |
| `Runtime.evaluate`       | `POST /navigate` + `GET /dom` |
| `DOM.querySelector`      | `POST /navigate` + guest queries |
| `Page.captureScreenshot` | (not ours for now)         |

Agents that already speak Playwright / puppeteer get namimado
support for free.

**New module**: `namimado::cdp_proxy`. **New CLI subcommand**:
`namimado cdp-proxy --port 9222`.
**Scope**: 1 session.

### J7. WebExtensions compat — `(defextension)` + manifest translator

**The mainstream way**: `manifest.json` + permissions + content
scripts + background workers.

**Our way**: translate a WebExtensions bundle into our `def*`
forms on install.

```sh
namimado extension install ./my-extension/
  # reads manifest.json
  # emits ~/.config/namimado/substrate.d/my-extension.lisp
  # copies background.js → ~/.config/namimado/wasm/my-extension.wasm
  #   (via JS-to-WASM — see J1)
```

Existing extensions absorb without the Chrome API surface.
**Scope**: 2 sessions (manifest parser + JS-to-WASM boot).

### J8. Paint + image / font pipeline — Blitz or lightweight

**The mainstream way**: Skia / WebRender / CoreGraphics.

**Our way**: optional pluggable renderer via trait.

```rust
pub trait CompositeRenderer {
    fn render_frame(&mut self, doc: &Document, layout: &LayoutTree) -> Result<Frame>;
}
```

Implementations:

- `GlyphonText` — our current (text only, no paint).
- `BlitzRenderer` — Blitz + vello (integrates CSS paint + images).
- `ServoRenderer` — libservo embed (full web fidelity).

Namimado picks at build time via feature.

**Scope**: 3 sessions (Blitz integration is big).

### J9. Accessibility tree — `(defax-emitter)`

**The mainstream way**: UIAutomation / AX API, maintained separately
from the DOM.

**Our way**: the **canonical `n-*` vocabulary IS the accessibility
tree**. `n-article` → `article` role, `n-nav` → `navigation` role,
`n-button` → `button`, `n-dialog` → `dialog` + modal attrs, etc.
A `(defax-emitter)` pass walks the post-normalize DOM and emits
a standard AccessKit tree.

```lisp
(defax-emitter :name "default"
               :backend :accesskit
               :role-map ((n-article article)
                          (n-button button)
                          (n-dialog dialog)))
```

Free accessibility for any site whose normalize packs we have
covered.

**Scope**: 1 session.

### J10. Cross-device sync — `(defsync)` over CRDT

**The mainstream way**: proprietary cloud sync.

**Our way**: state cells + bookmarks + history ride on a
user-chosen CRDT backend. Tatara already has event-bus primitives.

```lisp
(defsync :name "user-profile"
         :cells ("bookmarks" "history" "visits" "reader-mode-on")
         :backend :nats
         :topic "nami.profile.user-id")
```

**Scope**: 2 sessions; depends on J4.

### J11. WebGPU / WebGL — out of scope for V1

No plans to absorb. Pages doing GPU compute today ship `.wasm`
anyway; the WASM host gets `wasi:gpu` when that spec stabilizes.

---

## Ordered roadmap

1. **J4** tabs/history/bookmarks — highest UX ROI, already scaffolded
2. **J3** defstorage — foundational for J4, J5, J7
3. **J5** content blocking with uBlock packs
4. **J9** accessibility tree (bonus: AccessKit makes screen readers work)
5. **J6** CDP compat proxy — ecosystem opens up
6. **J1** JS runtime (boa or QuickJS-WASI)
7. **J2** service workers (needs J1)
8. **J8** Blitz/vello compositor
9. **J7** WebExtensions compat (needs J1 + J8)
10. **J10** cross-device sync (needs J4)

Each step adds a typed DSL, a `(def*)` keyword in the typescape,
a Rust trait where extension points matter, tests, and a pack or
two to prove the absorption works against real sites.

## Invariants we maintain through every absorption

1. **No ambient code.** Default caps are read-only; everything
   destructive is an explicit grant.
2. **Typescape stays the source of truth.** Every new capability
   lands in `nami_core::typescape` + `namimado::typescape` so
   `GET /typescape` enumerates it.
3. **Provenance chains don't break.** `data-ast-source` +
   `data-n-from` + `data-n-rule` survive every new pass.
4. **Same DSL, multiple source languages.** Normalize rules keep
   folding HTML + JSX + Svelte + (soon) Vue + SCSS into the
   canonical `n-*` space.
5. **BLAKE3 attestation holds.** The typescape hash changes
   deterministically with each new capability, so drift is
   CI-detectable.

This is how we absorb without becoming another Chromium clone.
