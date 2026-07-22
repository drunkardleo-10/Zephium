# Zephium Architecture

FOSS browser on the OS-native webview (Tauri + Wry), Rust-heavy, with a SolidJS
UI and no application-owned telemetry. This document is the single source of
truth for structure and boundaries. Revised 2026-07 during the foundation
review; it reflects both what is built and the agreed target. Security
enforcement lives in
`docs/security-model.md`.

Rigor is spent only where a decision is expensive to reverse; everything else
stays flexible.

---

## 0. North star and constraints

**Power, polish, and customization without shipping another browser engine.**
The distribution target is roughly 15–20 MiB, but size never overrides native
safety, data durability, or release authenticity. A dependency must justify
both its shipped bytes and its runtime behavior; large engine/runtime payloads
are not bundled merely to meet a feature deadline.

The constraints that remain hard:

1. **Runtime cost**: baseline memory, CPU, startup latency. The browser must
   feel weightless and instant on weak machines. Judge every addition by hot-path
   and baseline-memory cost, not by disk: a Rust crate is ~0 at runtime (add
   freely); an extra live webview is real RAM (count it).
2. **Security** (priority #1, see security-model.md).

Mental model: Arc (and Zen) for product shape, Vivaldi for customization depth,
plus our own: Raycast-style global launcher, notes/tasks/easel, site-as-app
windows, local data visualization. Feature surface is intentionally large; the
architecture below exists so features land as "just work" on top, not as
rewrites.

---

## 1. Architecture style (named honestly)

A composite of independent patterns, each applied where it pays:

1. **Functional core, imperative shell (FCIS).** The domain is pure data + logic
   with zero I/O. All side effects live at the edges.
2. **Single source of truth + unidirectional flow.** Authoritative state lives in
   the Rust core; every UI surface (chrome, overlays, internal pages) is a
   projection that sends intents. Multi-window and session restore fall out for
   free.
3. **Command/event core with domain slices.** Core state is composed of domain
   slices (see §6), each with its own commands, reducer and events. One actor
   loop in `zephium-app` owns the composed state: commands in (queued messages),
   pure reduction, effects out through ports, events out to projections. Adding a
   feature = adding a domain slice; existing domains are not touched.
   Cross-domain operations (close tab -> Items + Splits + Session) compose
   reducers inside one command, atomically, on the single owner thread.
4. **Ports and adapters, applied pointwise and lazily.** Traits abstract I/O
   edges where a port earns its keep (§4).
5. **Event sourcing: deferred.** One cheap discipline is kept: every mutation to
   a persistable aggregate goes through its reducer (no out-of-band writes). That
   chokepoint is where a change-log for sync attaches later, without a rewrite.

The expensive decisions are 1-3 (purity, ownership, command/event shape).
4-5 evolve with the code.

---

## 2. Process and trust model

The native webview delegates renderer parsing/JIT and process sandboxing to the
maintained OS engine: untrusted web content normally runs in engine-owned
content processes we did not build. Zephium still has to prove each supported
runtime's confinement and keep page data out of its Rust/native capabilities;
using a multi-process WebView is not itself a site-isolation guarantee. Rust
requests page-world scripts for tightly bounded observations, but never treats
their results as trusted code or data.

Three webview classes, mapping to the trust zones in security-model.md:

```
+- OS-sandboxed content processes (UNTRUSTED, zone 3) ----------------+
|   raw wry child webviews. web pages. ZERO IPC bridge by construction |
+---------------^------------------------------------------------------+
                | FFI (engine adapter, the only unsafe surface)
+---------------+------------------------------------------------------+
| Main process (TRUSTED, zone 1) - Rust core                           |
|   core domains . actor shell . ports . adapters . composition root   |
+---------------^------------------------------------------------------+
                | typed IPC (specta-generated) / typed overlay bridge
+---------------+------------------------------------------------------+
| Our UI webviews (SEMI-TRUSTED, zone 2)                               |
|   chrome (Tauri main webview) . overlay surfaces . internal pages    |
|   local bundled assets ONLY; no authoritative state                  |
+----------------------------------------------------------------------+
```

Attack surface we own: the IPC/bridge boundary, parsers of semi-trusted input
(filter lists, URLs, themes, config, extension manifests), FFI to the webview,
and any purpose-built application downloaders added for trusted browser data
such as filter lists or signed updates.

v1 is a single main process with async tasks; the untrusted code is already
process-isolated by the engine.

---

## 3. Native shell: tiling, stage, windows, overlays

These decisions are proven in code and are load-bearing. Do not revisit without
re-reading this section.

**The cursor-fight iron law (macOS).** Two overlapping interactive WKWebViews in
one window fight over the cursor: NSTrackingArea fires regardless of z-occlusion
(clicks hit-test with z; cursor tracking does not). Therefore:

- **Tiling, never overlap.** Chrome = one region rectangle (sidebar or top bar),
  content = another, gaps = window vibrancy. `core::layout` computes both rects
  from (window size, mode, metrics).
- **Native content stage owns content geometry.** A custom NSView subclass
  (objc2 `define_class`) hosts content webviews as subviews and lays out the
  split tree inside AppKit's layout pass (`resizeSubviewsWithOldSize:`), so
  window/split resize is smooth with zero Rust per frame. Split dividers are
  dragged natively in the stage; ratio changes sync back to core via
  `EngineEvent::SplitChanged`. Windows/Linux get the equivalent (§13).
- **Resize/layout is latest-value and revision-exact.** The shell coalesces
  window-size facts, the engine retains at most one newest layout per bounded
  window, and each stage hides removals before additions. Windows caches native
  geometry/visibility and applies only deltas; COM visibility is revalidated
  around each re-entrant call. Linux performs at most four immediate GTK
  convergence attempts and owns one coalesced idle retry. macOS gives every
  content update a container epoch before the first AppKit call, while its
  native split tree recomputes pane frames during resize. A stale pass may hide
  content, but it may never reveal content for an older tab/split revision.
- **Chrome is positioned natively** (setFrame + autoresizing mask), NOT via
  Tauri multiwebview `add_child` (behind the `unstable` flag). Boring permanent
  OS API over unstable framework feature.
- **Window content size is read from the chrome webview's superview bounds**,
  not `inner_size()` (contaminated after the chrome webview is shrunk).
- Non-interactive visuals over content (drop indicator, zone highlight) are
  plain layer-backed native views: no tracking area, no cursor fight. Proven by
  the DnD indicator.

**Multi-window is first-class.** `Window` is a core entity; each window has its
own chrome presence, stage, mode and split tree, all projecting the one
authoritative state. Window kinds: `Main` (full chrome), `App` (site-as-app,
minimal/no chrome), `Little` (ephemeral Arc-style link window).

**Overlays.** DOM cannot render over a content webview, so anything that
visually crosses the chrome/content boundary is its own surface. Preference
order: native menu > overlay panel > DOM.

- **Native first for menu-shaped UI.** Context menus, dropdowns and selects are
  native menus (Tauri Menu popup): zero webview cost, correct z-order, system
  behavior. Simple confirmations are native dialogs/sheets. DOM renders only
  what stays fully inside the chrome region (tooltips, hovers).
- **One reusable overlay panel** hosts the launcher/palette (and later find-bar,
  rich modals): currently a pre-warmed hidden Tauri WebviewWindow configured
  as a floating, all-Spaces auxiliary NSWindow on macOS. It is never changed
  into an unrelated Objective-C class after allocation. Being a Tauri window,
  it reuses the specta IPC and asset protocol as-is; the frame routes by
  window label. The pool grows to 1-2 recycled surfaces only when two must
  coexist.
- **The launcher is ONE surface with one behavior**: a reusable auxiliary
  window shown by both focused and global shortcuts. X11, Windows and macOS
  place it near the top-center of the active monitor. Native Wayland exposes
  no global coordinates, so the compositor owns its final placement.
  Allocation-time non-activating NSPanel semantics require a supported
  Tao/Tauri constructor seam and are not claimed by the current NSWindow.
- Anchored overlays (find-bar, modals over content) attach the same panel as a
  child window of the main window and position relative to its frame.
- Per-platform backends: macOS auxiliary NSWindow (with a future
  allocation-time NSPanel seam); Windows tool window plus the native global
  shortcut adapter; Linux uses a separate GTK Tauri window. X11 uses an
  asynchronously installed, reply-checked direct grab and free positioning.
  Its worker verifies detectable XKB repeat behavior, derives active lock masks
  from the server keymap, and publishes capability only after every grab is
  acknowledged; connection or mapping changes immediately retire the stale
  grab and restore the focused fallback. Direct X11 global registration is
  currently limited to layout-independent Space/Tab keys. Configured text keys
  remain available through the focused GTK path rather than being guessed
  across XKB groups/levels.
  Native Wayland uses the XDG GlobalShortcuts
  portal, accepts only the exact returned binding/session, and consumes the
  portal activation token before presenting the window. The worker watches
  the portal owner before Registry admission, pins subsequent calls to that
  exact unique D-Bus owner, and retries a bounded three times on restart or a
  bounded non-interactive call timeout. An explicit Registry policy or
  identity rejection is terminal for that worker run: replaying the same
  identity against the same owner cannot repair missing desktop metadata and
  would only delay the focused fallback. A user-interactive Bind response is
  never timed out. `ShortcutsChanged` is subscribed before the authoritative
  List/Bind exchange; removal or malformed state revokes the global-capability
  bit before parsing so the focused fallback resumes immediately. If the
  portal is absent, rejects the exact Registry identity, is denied, times out,
  restarts, or closes the session, the
  focused-window shortcut remains available in both the main and panel
  windows and no background-shortcut capability is claimed. Passing an XDG
  `parent_window` handle remains future, device-tested permission-dialog UX
  hardening; it is not required for capability correctness.
- Packaged Linux identity is one exact value: Tauri's Linux-only product name,
  installed desktop basename, GTK application id, and host Registry id are
  `app.zephium`; a custom desktop template keeps the visible `Name=Zephium`.
  The release workflow verifies the extracted RPM entry before signing. A raw
  development binary has no installed desktop-entry proof, so its attempted
  Registry identity may be rejected and cannot be treated as proven. Zephium
  never falls through to automatic cgroup identity after such a rejection; the
  focused fallback is the dependable development path.
- A future true NSPanel must be allocated with a Tao-compatible subclass and
  layout from the start; changing a live TaoWindow to a sibling Objective-C
  class is forbidden. A raw zone-2 Wry view in an owned panel remains another
  possible implementation, with local assets only and Rust-validated commands.

**Internal pages** (history, settings, graphs/data viz, notes editor, easel):
zone-2 webviews placed INTO the stage content region in place of a content view.
The stage hosts either zone-3 content or a zone-2 internal view for a given
pane; internal views are navigation-locked to bundled assets. This gives
arbitrarily rich internal UI with no new mechanism.

---

## 4. Workspace layout

Single Cargo workspace monorepo, frontend included. Per-platform native code is
`cfg`-gated inside the owning crate, not split into per-OS crates.

```
/
├── Cargo.toml                   [workspace]
├── deny.toml                    FOSS license allowlist, bans, advisories
├── rust-toolchain.toml          pinned toolchain
│
├── crates/
│   ├── zephium-core             domains (slices + reducers + commands/events),
│   │                            layout/split math, navigation policy, ports.
│   │                            pure, #![forbid(unsafe_code)], zero I/O.
│   ├── zephium-ipc              DTOs + specta/tauri-specta TS codegen.
│   ├── zephium-engine           Wry adapter + native stage per platform
│   │                            (stage_macos / stage_windows / stage_linux,
│   │                            same inherent surface, cfg-selected).
│   │                            unsafe allowlisted here.
│   ├── zephium-store            SQLite actor, migrations, FTS5, fakes.
│   └── zephium-app              the actor shell: owns composed core state,
│                                drives ports, builds projections.
│
├── desktop/                     Tauri composition root. windows, menus/hotkeys,
│                                overlay backends, platform wiring. ONLY crate
│                                that touches tauri.
│
├── frame/                       SolidJS app (Vite, Tailwind v4). One app,
│                                entry-routed: chrome | overlay | internal
│                                pages | onboarding.
│
└── xtask/                       dev automation. `cargo xtask ci` = the one gate.
```

**Dependency rules (CI-enforced):** core depends on no internal or I/O crate;
only `desktop` knows tauri; `unsafe` only in engine/native adapters with
`# Safety` notes. Page-derived network access stays inside the exact
profile-scoped engine session; the application has no generic URL-fetch port.

---

## 5. State and data flow

One direction. UI surfaces never own truth.

```
UI intent -> IPC/bridge -> command queue -> actor: domain reducer (pure)
                                              |
                              +---------------+----------------+
                              v               v                v
                        store persists   port effects    domain events
                                        (engine, ...)         |
                                                    UI surfaces reconcile
```

Engine callbacks (title, url, loading, favicon, nav state, new-window intent,
permission request, download start, crash, native-runtime restart requirement)
enter the same queue as commands. Runtime restart state is process-sticky and
is replayed on chrome bootstrap; it is never cleared by recycling one content
profile.

Main-frame navigation uses an opaque native identity rather than requested-URL
equality, so same-origin, cross-origin, and multi-hop redirects retain one
ordered lifecycle. The raw view is hidden at native commit before Rust can be
re-entered. Privileged chrome atomically applies and verifies the exact
revision-bearing final URL/title projection; the actor correlates that revision
again before it queues first-content geometry and returns the same opaque
presentation id. A fresh tab's real New Tab surface remains in place until this
transition, with no synthetic native placeholder. Native title attribution
begins only after the exact navigation finishes.

- **Projection is granular per domain** (`items.*`, `spaces.*`, `downloads.*`),
  NOT one monolithic snapshot: no re-render storms at scale. A fresh surface
  gets a domain snapshot, then ordered deltas.
- High-frequency streams (load/download progress) use a channel, not event
  spam; frequent deltas (title/favicon/loading) are coalesced.
- **Durability tiering.** Ephemeral session state persists debounced/async.
  Durable user data (bookmarks/pins, documents, settings) persists BEFORE the
  UI is told the action succeeded.
- Optimistic UI (drag-reorder) is transient view state, reconciled against the
  projection.

Privileged mutations first reserve a bounded process-local operation id, then
enter the non-evictable command FIFO. The actor records a typed disposition in a
1,024-entry desktop ledger before event delivery. Chrome subscribes before
calling `operations_reconcile`, deduplicates results, and retries explicit
acknowledgement; the backend refuses new admission rather than evict an
unacknowledged result. This repairs WebView reloads and missed events, but it is
not a durable cross-process queue. Only operations with their own persistence
journal, currently profile deletion, resume after process death.

**Sync patterns by cardinality:** bounded state (items, spaces, windows) =
snapshot + deltas; unbounded (history, downloads) = query + paginate, never
pushed; small config (settings, theme) = load once + change events.

---

## 6. Domain model (the part to get right)

Aggregates in `zephium-core`, invariants enforced inside the aggregate.

**ID strategy (irreversible).** Persistent/syncable entities (Profile, Space,
Item, Document, Setting, Theme, SearchEngine, permission grants) get ULIDs so
future sync never collides and the FK graph never needs rewriting. Runtime-only
entities (View, Window) use local `u64`. Deletes of syncable entities are
tombstone-capable through the reducer chokepoint.

**The sidebar is a tree of Items, not a flat tab list.** Arc model: a pinned
tab IS a bookmark that can be alive. This unifies bookmarks, pinned tabs,
favorites and folders into one aggregate; there is no separate bookmarks system.

- **Profile** - isolation root. `{ id, name, color, kind: Default | Named |
  Incognito }`. Owns a store partition (own SQLite) and a webview data
  partition (WKWebsiteDataStore / WebView2 user-data-folder /
  WebKitNetworkSession). Incognito is ephemeral, never persisted.
- **Space** (workspace) - `{ id, profile, name, theme, order }`. Switching
  spaces swaps the visible item set and the theme.
- **Item** - sidebar node. `{ id: Ulid, container, parent: Option<ItemId>,
  order, kind: Folder { name } | Tab { url, title, favicon } }`.
  Containers/sections: `Favorites(profile)` (profile-wide grid),
  `Pinned(space)`, `Today(space)` (ephemeral tabs, auto-archivable).
- **View** (runtime only) - a live webview bound to an item:
  `{ view: u64, item: ItemId, profile }`. Many items, few views (§7).
- **Window** - `{ id, kind: Main | App { origin } | Little, profile,
  active_space, mode, splits: Option<Pane> }`.
- **Layout / Splits** - `Mode` (Sidebar now; Horizontal/top reserved) + the
  split `Pane` tree (built, proven). Layout owns geometry; the engine
  positions native views to match.
- **Session** - snapshot of windows/spaces/splits/active per profile for
  restore. Restore rebuilds aggregates and replays the same projection path.
- **History** - append log per profile. Queried, not pushed.
- **Documents** - `{ id, kind: Note | Task | Easel, content (typed JSON),
  refs: [ItemId | Url] }`. Notes/tasks/easel/research live here; easel cards
  can hold page captures (§8 engine `capture`).
- **Downloads** - entries with state/progress; supervised subsystem.
- **Permissions** - per `(profile, origin)`; default deny.
- **SearchEngines / Settings / Themes** - data-driven; themes are token maps
  validated against a typed schema.
- **Commands** - registry with stable string ids (`tab.close`, `space.next`,
  `page.copy-markdown`). One registry powers native menus, accelerators, the
  palette, the launcher and the user-configurable keymap (Vivaldi-grade).
- **ContentRules** - compiled adblock/user-filter state per profile (§9).
- **SearchIndex** - FTS5 across items, history, documents and commands; feeds
  the launcher and palette (§8).

---

## 7. Tab lifecycle and view binding (un-retrofittable seam)

A logical tab (Item) is not a webview. Mapping is many items -> few views,
governed by a scheduler; 200 tabs must never mean 200 content processes.

```
New(empty) -navigate-> Active <-> Inactive -idle-> Hibernated -> Closed(restorable)
```

- New/empty tab has no webview (zero engine cost); the chrome renders the
  new-tab view. The webview is created lazily on first navigation (built:
  `view: bool` on Tab).
- Three tiers, built (shell = policy via idle clock + maintenance tick;
  engine = mechanism): hidden views get a low-memory hint on the visibility
  transition; hidden AND idle (5 min) views suspend where a primitive exists
  (WebView2 `TrySuspend`, auto-resume on visible; WebKit suspends hidden
  processes itself). Twelve live views is the soft warm target. Above it,
  hidden pages idle for 15 minutes may enter an exact generation/navigation
  discard-safety probe; above the pressure watermark of 24, eligible hidden
  pages may be probed without that long-idle grace. At most four probes run at
  once, visible split leaves are protected, and an uncertain page is never
  force-discarded. A successful discard drops the view but keeps the item;
  activation recreates it and reapplies zoom.
- The application refuses a 33rd logical view synchronously. Its absolute 32
  ceiling includes eight slots for the largest visible split/recovery batch.
  The native engine has an independent ceiling of 48 counting live views, a
  warm spare, construction reservations, and WebView2 cleanup debt that may
  still own a controller. These constants are admission bounds; the packaged
  1/10/50/100-tab and 24-hour resource measurements remain release work.
- Discard fidelity upgrade is planned per-engine: WKWebView
  `interactionState`, WebView2 resume-state, WebKitGTK
  `WebKitWebViewSessionState` (back/forward list). Until then a discarded
  tab restores by URL.
- Inactive non-discarded tabs keep their webview hidden (media, sockets,
  scroll survive a switch).

---

## 8. Engine port v2

The full surface is declared now; implementations land incrementally. Grouped:

- **lifecycle**: `create_view(view, profile, url, opts)`, `close`. Views are
  created against the profile's data partition.
- **nav**: `navigate`, `reload`, `stop`, `back`, `forward`.
- **layout**: `set_content(window, tree, region)`, `set_drop_indicator(zone)`.
- **page ops**: `capture -> png` (easel, previews), `extract_html` (html->md,
  research), `find`, `zoom`, `mute`, `print_pdf`.
- **injection**: `set_user_content(scope, scripts, styles)` where scope is
  profile or origin. Designed from day one with **isolated worlds and a
  two-way messaging channel**, because the same pipeline serves the built-in
  scrollbar CSS, Boosts (per-origin page customization), userscripts/
  userstyles, adblock cosmetics, and a future extensions layer (§10).
- **rules**: `set_content_rules(profile, compiled)`; plus an **optional
  request-level hook** where the engine supports it (WebView2). On engines
  without it, rules stay declarative; the port models both.
- **events**: Title/Url/Loading (built), SplitChanged (built), Favicon,
  NavState{can_back, can_forward}, NewWindowRequested{url, disposition},
  PermissionRequested, DownloadRequested, Crashed.

Session restore state (§7) rides on lifecycle (`opts` carries restore state).

The built favicon path deliberately has no second Rust HTTP stack or native
image decoder. The exact site renderer scans a bounded set of same-origin icon
links plus `/favicon.ico`, decodes there, and returns only canonical 32x32 RGBA
for the current view generation/navigation epoch. Seven timed polls are
followed by at most one fresh pass at document completion when the initial
budget expired during loading. Cross-origin/CDN icons are not fetched until a
profile-scoped broker can share the engine's exact cookies, proxy, DNS, and
shutdown policy; affected sites may show the fallback rather than their
declared icon.

Future application-owned downloads, such as maintained filter lists or signed
updates, are not page-derived browsing traffic. Each must use a purpose-built,
bounded component with its own source allowlist, redirect, proxy, integrity,
retention, and shutdown policy. A generic native fetch seam must not be
reintroduced for page-controlled URLs.

---

## 9. Blocker (planned next track, adblock-rust)

The foundation keeps a policy port for a future built-in blocker based on
Brave's `adblock-rust`; the current `set_content_rules` adapter is a no-op and
no blocking claim is made. The intended implementation uses two architectures
behind one policy port, never per-request logic in core:

| Platform | Network blocking | adblock-rust role | Cosmetic |
|---|---|---|---|
| WebView2 (Win) | runtime intercept (`WebResourceRequested`) -> cancel/redirect | full runtime matcher (`$redirect`, scriptlets); Chromium is adblock-rust's home turf | inject CSS/JS at document start |
| WKWebView (mac) | declarative `WKContentRuleList` | list parsing + `content_blocking` conversion to WebKit JSON + cosmetics | `WKUserScript` via injection pipeline |
| WebKitGTK (Linux) | declarative `WebKitUserContentFilter` (same JSON format) | same as macOS | injected stylesheet/script |

Expected, documented gap: no runtime `$redirect`/scriptlets on WebKit;
declarative rule-count caps (split large lists). Cosmetic injection runs before
render to avoid ad flash. List pipeline: purpose-built allowlisted downloader ->
parse/validate (fuzz later) -> compile -> cache per profile -> hot reload. A
local-root-CA MITM proxy is rejected: unacceptable trust liability in a privacy
browser.

---

## 10. Extensions (tiered; NOT impossible, priced honestly)

**The WebExtensions API is a browser-layer API, not an engine API.** Chrome and
Firefox implement `chrome.*`/`browser.*` in the browser on top of engine
primitives. Proof on our exact engines: Safari ships Web Extensions as an
app-layer implementation on WebKit; Orion (Kagi) reimplemented ~70% of the API
surface on WebKit over years of dedicated work; GNOME Epiphany has an
experimental layer on WebKitGTK. So it is feasible, and it is expensive: an API
surface of hundreds of methods with permanent parity-chasing.

Strategy, in order:

- **Tier 0 (foundation, in the phases below): built-ins.** adblock (§9) +
  userscripts/userstyles (Tampermonkey class) + Boosts, all on the one
  injection pipeline. This covers the top reasons people install extensions.
- **Tier 1 (cheap, after the Windows adapter): native Chrome extensions on
  Windows.** WebView2 `CoreWebView2Profile.AddBrowserExtension` loads unpacked
  Chrome extensions into the profile: real Chromium extension runtime (content
  scripts, background, webRequest). WebView2 hosts no extension UI; we host
  action popups/options in our overlay surfaces. Install = fetch .crx (zip),
  unpack, add; persists per profile.
- **Tier 2 (own phase, after core features): compat subset for macOS/Linux.**
  Data-driven: take the top ~100 extensions, implement the API subset they
  need (storage, runtime, tabs, scripting, cookies, contextMenus,
  declarativeNetRequest). `chrome.*` vs `browser.*` is one implementation
  behind a polyfill shim. Position like Orion: "supports popular extensions",
  never promise 100%.

Ceilings, stated plainly: public WKWebView has no blocking webRequest (Orion
uses a custom WebKit build; we will not maintain an engine fork), so
MV2-uBlock-class network extensions are out of reach on macOS/Linux; our
built-in blocker is the answer there. The password-manager / Dark Reader /
content-script class (the majority of real usage) fits within public
primitives.

**Architecturally, extensions are a future consumer of ports that already
exist** (injection + isolated worlds + messaging, content rules, storage,
overlay surfaces for popups, commands registry). No foundation change is
required to keep this door open; the isolated-world messaging design in §8 is
the one deliberate provision.

---

## 11. Frontend (frame)

SolidJS + TypeScript + Vite. A pure projection; only transient view state.

- **One app, entry-routed by surface**: chrome (sidebar/topbar), overlay
  (launcher, palette, find, floating panels), internal pages (history,
  settings, graphs, notes, easel), onboarding. All share components, tokens
  and stores.
- **Styling:** Tailwind v4, token-first. Design tokens are CSS custom
  properties and the single source of truth; runtime theming (Vivaldi-grade
  customization) = swapping variable values, driven by the Themes domain.
  Kobalte for headless accessible primitives; the look is ours.
- **State:** store-per-domain, updated by granular domain events (§5).
- **Command mirror:** the frontend consumes the Commands registry; palette,
  launcher, menus and keybindings all dispatch the same stable command ids.
- Structure by feature: `app/ features/ ipc/ (generated) theme/ ui/ styles/`.

---

## 12. IPC contract

Typed end-to-end with **specta + tauri-specta** (commands, events, types
generated from Rust; CI fails on drift). Namespaced by domain (`items.*`,
`spaces.*`, `downloads.*`), not a flat list. Overlay surfaces use the thin wry
bridge with the SAME specta-generated types (one contract, two transports).
Commands validate inputs in Rust; errors are a serializable `AppError`.
Adding a command touches the capability manifest: a small, reviewable security
checkpoint.

Mutating commands return an `OperationAdmission` and later emit a correlated
`OperationDisposition`; IPC must not discard either admission failure or a
backend/store rejection. `Deferred` means the actor queued native/store work,
not that navigation, reload, or rendering completed. Native-runtime update state uses the stable typed
`zephium:runtime-status` event so UI presentation can be added without changing
the backend contract.

---

## 13. Platform layer

Development reality: all three OSes are available for real testing, including
Linux under Wayland (GNOME, Hyprland) and X11 (i3). Native code is written
against the OS it runs on; no blind ports. CI matrix keeps all targets
compiling (§14). The current source-level/native checks do not replace the
packaged hostile-page and endurance matrix still required on real machines.

Per-OS map for the native seams (each a `cfg`-selected module with the same
inherent surface; no `dyn`):

| Seam | macOS (built) | Windows | Linux |
|---|---|---|---|
| Webview handle | `WebViewExtMacOS` -> WKWebView/NSView | `WebViewExtWindows` -> ICoreWebView2Controller | `WebViewExtUnix` -> webkit2gtk WebView |
| Chrome positioning | setFrame + autoresizing mask | `put_Bounds` on WM_SIZE | `gtk::Fixed` move + size |
| Content stage | NSView subclass, in-pass layout | child HWND + WndProc subclass on WM_SIZE | `gtk::Fixed`/`Layout` + `size-allocate` |
| Divider drag | mouseDown/Dragged/Up | WM_LBUTTON*/MOUSEMOVE | button/motion events |
| Drop indicator | layer-backed NSView | layered child HWND | GtkDrawingArea overlay |
| Window material | NSVisualEffect (window-vibrancy) | Mica/Acrylic (window-vibrancy) | solid (no portable blur) |
| In-app overlay | auxiliary NSWindow | Tauri tool window | separate GTK Tauri window |
| Global launcher | auxiliary NSWindow + native shortcut | tool window + native shortcut | X11 direct grab / Wayland XDG portal; compositor-owned Wayland placement |
| Content size | superview bounds | client RECT | GTK allocation |
| Per-pane corner radius | layer cornerRadius | skipped (no clean per-control rounding) | GTK CSS / cairo clip |

Bindings, direct per platform (no cross-platform UI abstraction layer):
objc2 + objc2-app-kit/foundation/web-kit/quartz-core (built); `windows-core` +
`webview2-com` matching wry's version; `gtk` + `webkit2gtk` matching wry's.
All already in the dependency tree transitively via wry; declared explicitly,
target-gated. Linux caveat: content views on Linux are created via
`WebViewBuilderExtUnix::build_gtk` into a `gtk::Fixed` owned by the
composition root (wry positions children only inside a Fixed;
raw-window-handle child-building is not the GTK path).

Current foundation status: seams live in `zephium-engine/src/platform/` and
`desktop/src/platform/` (cfg-selected modules, one per OS, same inherent
surface). All three implement chrome positioning, content layout, native
navigation probes, and per-profile engine partitions. On macOS the overlay
keeps Tao's allocation-time Objective-C class, while queued chrome layout uses
a main-thread-retained, generation/attachment-checked WKWebView that is
unpublished before release. On Windows, every fallible controller build owns a
typed cleanup plan for the parent subclass, controller `Close`, and child HWND;
unresolved steps return as retryable profile-attributed debt and consume native
resource budget. On Linux, context properties fail closed and CI additionally
spawns a real Fedora WebProcess to inspect namespaces, no-new-privileges,
seccomp, and host-path denial. Packaged real-OS hostile tests are still a
release gate. Split-divider drag/drop indicators on Windows/Linux and per-view
rounded corners remain later platform work (rounded corners are intentionally
skipped on Windows for now).

---

## 14. CI

`cargo xtask ci` is the local gate (fmt, clippy, tests, frame check).
GitHub CI is the full matrix: {windows, macos, linux} native runners so
`cfg`-gated code cannot rot, plus nextest, bindings-drift, coverage, and
advisory `cargo machete` / `cargo deny check` (licenses, advisories,
sources; deny.toml at the root). Frontend gate: `tsc --noEmit` + biome.
The supported Fedora job explicitly runs the otherwise ignored real-WebProcess
confinement probe; its result applies to that CI image, not every installation.
Later: sustained fuzzing for new parsers (filter lists, themes), `cargo geiger`,
packaged hostile tests, and endurance/resource gates.

---

## 15. Build order

Phase 0 is the only "rewrite" step in the plan; it exists so phases 1-5 do not
rewrite anything. Everything already proven (split math, stage, tiling, DnD,
chrome positioning) carries over as-is.

- **Phase 0 - foundation:** actor core + domain slices; ULIDs + Item tree +
  Window entity; migrations + FTS5; specta + granular per-domain events;
  Engine port v2 signatures; CI matrix + platform seam stubs.
- **Phase 1 - input and overlays:** Commands registry + native menus/
  accelerators + keymap; overlay system (macOS backend first); launcher v1 +
  palette.
- **Phase 2 - product model:** Item tree UI (pinned/folders/favorites),
  Spaces, Profiles (incl. incognito).
- **Phase 3 - cross-platform:** Windows adapter complete (chrome positioning,
  stage, dividers, indicator, overlays), then Linux (GTK stage + GtkOverlay +
  Wayland specifics), each verified on real hardware.
- **Phase 4 - table stakes:** downloads + MOTW, find-in-page, permissions
  prompts, history UI, full session restore incl. splits.
- **Phase 5 - differentiators:** Boosts + userscripts/userstyles, notes/
  tasks/easel, site-as-app windows, adblock wiring per §9, Tier 1 extensions
  (Windows), data viz internal pages, html->md and clipboard commands.
- **Later, own track:** Tier 2 extensions compat, sync (change-log at the
  reducer chokepoint), SQLCipher/SecretStore.

---

## 16. Reversible vs irreversible

**Irreversible - get right now:** domain model shape incl. Item tree and
Window (§6); state ownership (§5); logical tab != webview, scheduler seam
(§7); per-profile isolation + schema (§8 persistence, §6 Profile); ID
strategy (§6); tiling + stage + overlay physics (§3); injection pipeline
with isolated worlds + messaging (§8).

Each irreversible decision gets an ADR in `docs/adr/` so a future contributor
or agent does not unknowingly revert it.

**Reversible - evolve freely:** number of ports, crate boundaries, module
splits, CI depth, exact reducer composition.

---

## 17. Persistence

`zephium-store`, SQLite via rusqlite (the plain `bundled` feature ships
FTS5; a test guards it).

- **Storage is an actor**: rusqlite is blocking; a storage actor owns the
  shared metadata connection and per-profile connections and serializes
  writes; the async Store port is a message send.
- **The isolation scope is explicit**: durable history and favicon data live
  in per-profile databases. The registry, application settings, and complete
  restorable non-private session share `meta.sqlite`; profiles are therefore
  not separate file-level principals.
- **Incognito is rejected at the persistence adapter**: it is absent from the
  registry, session snapshot, history, and favicon databases. This is not a
  claim that native web engines, swap, crash artifacts, or the OS never write
  temporary private-session bytes to disk.
- **Forward-only versioned migrations** (`user_version`) from day one, each
  with an up-test on representative data. A schema newer than this binary is
  rejected, so rollback cannot write through a future format. The claimed
  version must also match a bounded full `sqlite_schema` manifest generated
  from the exact immutable migration prefix before migration DML and after
  every step; injected/replaced triggers, indexes, views, tables, and FTS
  shadow objects therefore fail closed instead of executing during migration.
- **Database-path admission rejects existing aliases.** The store pins its
  canonical data root, requires regular single-link files, creates new files
  exclusively with owner-only permissions, opens SQLite with `NOFOLLOW`, and
  rechecks platform file identity after open. This is not confinement against
  a malicious process already running as the same OS user: closing all
  directory-component and WAL/SHM races would require descriptor-relative
  filesystem operations and persistent ownership identity end to end.
- **Ancillary corruption degrades one profile, not the browser.** Once an
  exact authoritative meta snapshot exists, every registered profile database
  is securely opened read-only and schema-verified before read-write setup.
  A future/altered schema is left untouched; any failed ancillary database is
  explicitly reported by profile ID and disabled without retry or recreation.
  The session and healthy siblings remain usable. Without authoritative session
  truth, the same condition still stops startup. Only the crash-resumable
  profile-deletion journal can later authorize removal of the preserved file.
- **Authoritative snapshots are exact, not repaired on read.** An
  allocation-free lexical preflight bounds JSON strings, scalar tokens,
  structure, and depth before schema-aware bounded visitors allocate profile,
  space, item, or split collections. Unknown fields and any difference from
  the canonical session are quarantined; the original row is preserved and
  the store stays read-only pending an explicit recovery flow.
- **Profile deletion is a two-phase, crash-resumable application workflow.**
  One SQLite transaction commits the exact survivor snapshot and authorization
  journal before the in-memory tombstone/native retirement. Ambiguous
  authorization rechecks the journal and rebuilds from the current
  process-local session revision before a legal retry. Native verification is
  durably recorded before the authorized profile database is unlinked; the
  journal clears only after success and startup resumes unfinished phases.
  Packaged tests of all engine storage types and crash boundaries remain a
  production gate.
- The current schema contains the shared registry/settings/session snapshot
  and per-profile history/favicons with FTS5. Additional browser stores remain
  future work, not an architectural guarantee.

---

## Non-negotiables

1. The domain stays pure and `unsafe`-free.
2. No UI surface ever holds authoritative state.
3. All mutations to persistable aggregates go through their reducer.
4. Only `desktop` knows Tauri. Page-derived network access stays inside the
   exact profile-scoped native engine; app-owned downloads require dedicated
   policy components rather than a generic fetch port.
5. Content webviews are NEVER given a bridge (see security-model.md).
6. `unsafe` only in engine/native adapters, each block documented.
7. In-app overlays position relative to the main window, never absolute.
8. A change is not done until `cargo xtask ci` is green on all three targets.
