# Zephium Architecture

FOSS browser on the OS-native webview (Tauri + Wry), Rust-heavy, with a Svelte 5
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

The current three webview classes map to trust zones 1-3 in
security-model.md:

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

Extension execution does not inherit the application-WebView trust zone. Each
installed extension has one non-reusable native principal. Content scripts may
execute only in engine-provided isolated worlds scoped to that principal.
Extension-owned background and UI surfaces form a separate unprivileged trust
zone: they receive no generic Tauri/Wry IPC bridge and no generic custom
protocol. Any enabled native registration must bind the profile, install,
principal, and generation; JavaScript payloads can never select or assert that
identity. Only capability-specific, permission-checked brokers may cross into
the trusted process. Raw page worlds remain bridge-free. This contract is
ratified for the extension architecture, but ordinary product builds keep
extension execution disabled until every platform adapter and hostile native
gate enforces it.

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
│   ├── zephium-blocker          bounded adblock-rust compiler worker and
│   │                            immutable platform artifacts.
│   ├── zephium-blocker-update   fixed-origin TUF authentication, private
│   │                            package storage, rollback/clock authority.
│   ├── zephium-blocker-service  candidate preparation, durable commit, and
│   │                            exact compiler activation coordinator.
│   ├── zephium-store            SQLite actor, migrations, FTS5, fakes.
│   └── zephium-app              the actor shell: owns composed core state,
│                                drives ports, builds projections.
│
├── desktop/                     Tauri composition root. windows, menus/hotkeys,
│                                overlay backends, platform wiring. ONLY crate
│                                that touches tauri.
│
├── frame/                       Svelte 5 app (Vite, Tailwind v4). One app,
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
- **Permissions** - page permissions are per `(profile, origin)`; extension API,
  host, temporary, and scheme grants are a separate per-`(profile, extension)`
  authority model. Everything defaults deny. A Chrome match pattern is never a
  permission grant, and local-file access requires an independent explicit
  grant even when the pattern is `<all_urls>`.
- **SearchEngines / Settings / Themes** - data-driven; themes are token maps
  validated against a typed schema.
- **Commands** - registry with stable string ids (`tab.close`, `space.next`,
  `page.copy-markdown`). One registry powers native menus, accelerators, the
  palette, the launcher and the user-configurable keymap (Vivaldi-grade).
- **ContentRules** - compiled native network-policy generation per profile
  (§9).
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
- **injection**: `set_user_content(scope, generation, content)` where
  `ContentScope = Global | Profile` is ownership, not URL targeting. `Global`
  is host-only; runtime callers may replace only a profile scope. Every
  registration has a stable owner-qualified `(owner, ScriptId)` identity,
  bounded `MatchSet`, and frame policy; scripts additionally carry `World` and
  `RunAt`. `MatchSet` expresses bounded URL eligibility, not authority; the
  permission broker must intersect host and scheme grants before installation.
  A terminal settlement reports the requested generation as applied, retained
  with the prior generation, or unavailable. The current adapters fail closed
  for live-view mutation and all principal-owned content until exact pre-source
  per-frame match enforcement, installed-state preservation, and isolated-world
  gates pass. Dormant macOS/Linux per-principal handler primitives are not
  retained by product construction; page worlds receive no native bridge.
- **rules**: `install_content_rules(profile, generation, compiled)` with an
  exact settlement event. Windows installs a synchronous request matcher;
  macOS/Linux install declarative native rules. Core models both as immutable
  policy payloads and never performs per-request matching.
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

## 9. Blocker (bundled network policy implemented; TUF next)

The native network blocker is implemented behind the core policy ports. The
desktop embeds an immutable, release-authenticated EasyList + EasyPrivacy seed
with exact license/provenance and compiler-quality manifests. Raw list bodies
are inflated only after a compiled-cache miss, and every new profile still
defaults to disabled. Enabling requires the exact authenticated catalog and
holds first navigation until a non-empty native artifact is installed; it
cannot present an empty policy as protection.

The production TUF trust domain is intentionally not provisioned yet.
Bundled mode is network-inert, exposes `release_bundle` provenance, and has no
manual source-refresh action. TUF will be a distinct, independently
authenticated authority rather than an environment-variable or writable
configuration override.

One bounded compiler worker parses already-authenticated ABP/uBlock-style or
hosts-format sources with the vendored `adblock-rust` fork. It publishes only
one native artifact for the current platform. The shell assigns an exact,
non-wrapping policy generation, holds first view creation/navigation until an
explicit allow-all or blocking policy is installed, and generation-checks
every worker and native settlement. A replacement is installed across the
profile's live-view cohort before the previous registration is retired;
ambiguous cleanup is terminal native-accounting failure.

| Platform | Implemented network mechanism | Deliberate v1 boundary |
|---|---|---|
| WebView2 (Windows) | frozen in-process matcher from synchronous `WebResourceRequested`; a block receives an empty, no-store 403 response | only document-sourced native stylesheet, image, media, font, script, XHR, fetch, and ping contexts; no document/subdocument/WebSocket/object/other interception and no service/shared-worker request interception; missing initiating-frame attribution uses conservative source-independent matching |
| WKWebView (macOS) | canonical WebKit JSON compiled/cached as `WKContentRuleList` and installed per raw view | conversion losses are counted; `$important`, method predicates, full regexes, and other non-equivalent rules are omitted rather than approximated silently |
| WebKitGTK (Linux) | the same canonical JSON compiled/cached as `WebKitUserContentFilter` and installed per raw view | the same declarative coverage boundary as macOS |

v1 is network-only on every platform. Cosmetic filtering, scriptlets,
redirect resources, CSP mutation, URL-parameter rewriting, generic-hide
controls, and tag-driven policy are rejected before artifact publication.
There is no second Rust proxy or local-root-CA MITM path.

Published blocking artifacts carry a checked coverage report and an explicit,
nonzero post-control native blocking-entry count. This is a structural
admission invariant, not a claim that exceptions leave every entry reachable;
exceptions and platform-omitted rules are not counted as blocking entries.
Approximation is also typed by resource reachability, native request-source
kind, and initiating-document attribution, with a deduplicated aggregate.

The purpose-built updater and coordinator are implemented. Release mode
validates deterministic embedded gzip, canonical manifests, exact raw
length/digest/header provenance, and the approved license before compilation.
TUF mode authenticates a fixed-origin, licensed package into a private
content-addressed store with a durable clock/revision high-water, then keeps a
candidate distinct from current while the compiler prepares its exact
artifact. Only that exact prepared candidate may be durably committed and
activated; current/previous known-good state and candidate recovery survive
crashes. Compiled artifacts and native WebKit content-rule namespaces have
bounded, identity-safe caches and garbage collection.

Privileged main chrome receives only a revisioned focused-profile status and
bounded refresh, preference, and exact-generation retry commands. It exposes
public package revision/digest and aggregate coverage diagnostics, not profile
IDs, URLs, per-request decisions, list text, or native/parser strings. Raw
content views have no blocker command surface. TUF trust material, packaged
native enforcement tests, legal approval, external review, and
resource/endurance evidence remain stable-release gates. Exact limits,
release-seed measurements, failure semantics, native coverage, and release
gates are documented in
[`adblock.md`](adblock.md).

---

## 10. Extensions (bounded MV3 subset; currently disabled)

The WebExtensions API is a browser-layer compatibility product, not a feature
the native engines provide uniformly. Zephium does not claim extension
installation, API mediation, userscripts, or Chrome/Firefox compatibility in
the current tree. The exact initial compatibility target is one pinned
**Bitwarden Core** build with a written supported/degraded/unsupported matrix,
not an open-store or general-parity promise.

Delivery is layered and measured:

- declarative data uses native rules/storage and no persistent JS runtime, but
  still has compile, match, and memory cost;
- userscripts/userstyles lazily require a native world/handler registration per
  active principal in each eligible live content controller/view, plus an
  isolated JS context in each eligible frame/document; both cardinalities are
  explicitly capped; and
- the macOS/Linux MV3 compatibility subset requires a bounded Zephium-owned
  event-runtime view/context plus any chosen extension-UI or capability-specific
  offscreen resources; WebView2-native extension workers are engine-managed and
  require separate count admission and measured process-resource gates.

On macOS 15.4 and newer, Apple's public `WKWebExtensionController` stack is the
preferred native MV3 candidate. A feature-gated live probe proves controller
attachment before view construction, explicit host/private-data grants,
per-extension isolated worlds, frame matching, exact context unload/reload,
preservation of Zephium's protected scripts, MV3 background execution, and
same-principal extension-storage isolation across two persistent controller
namespaces and fresh controller instances. A second behavioral gate constructs
two regular profiles through the exact dormant product registry and one
nonpersistent private session. It pointer-attests each view/controller/store
binding, proves mutually exclusive cookie and extension-storage state, proves
regular reconstruction and private noninheritance, and publishes distinct
tab/window/delegate graphs to demonstrate that each controller exposes only its
own profile surface. The probe retires each namespace independently, reopens
both to verify zero persistent extension bytes, and requires every native
view/controller/context/store and routing object weak reference to release.

Native grant replacement is an exact, bounded main-thread operation. The pure
grant compiler supplies the complete allowed API/host set; the adapter clears
all four WebKit permission dictionaries, revokes every bounded prior key
through the per-key status API, applies every new key through that same API,
and accepts the generation only after exact dictionary/status/private-access
readback. This distinction is behavioral: bulk dictionary assignment alone
does not recompute content-script eligibility for a loaded context. The live
probe therefore proves host-grant activation, revocation, and restoration on
subsequent navigations, then unloads the context before clearing and verifying
final absence. Production native activation remains disabled until this
boundary is joined to authenticated package and operation authority.

An extension-origin page is not navigated in a normal profile view. WebKit
requires the loaded context's customized
[`webViewConfiguration`](https://developer.apple.com/documentation/webkit/wkwebextensioncontext/webviewconfiguration),
so toolbar popups and other extension UI use a separately budgeted view and a
controlled swap at the extension-origin boundary. One private extension context
per installed extension lives for the private-session lifetime; its UI views
may be recreated from that context, while destroying the session context,
controller, and nonpersistent store is the storage-erasure boundary.

These probes do not prove authenticated package admission, production
delegate/API mediation, product extension UI, quotas/endurance, or product
startup/crash reconciliation, so ordinary builds still expose no extension
runtime. Older admitted macOS versions and Linux require a Zephium
compatibility runtime only after per-principal world/handler isolation, exact
match enforcement, protected-script installed state, and native hostile tests
pass. Windows has two separate candidates: curated MV3 packages through a
production wrapper around
`AddBrowserExtension`, whose environment-level enablement is a startup-time
decision, and a CDP isolated-world probe for first-party userscripts. The CDP
probe is excluded from normal product builds and has no page-world fallback;
its lifecycle, removal, navigation, resource, debugger-coexistence, and live
performance gates remain open.

This work needs new foundations rather than merely consuming existing ports:
a profile-scoped permission broker, per-install extension scheme and
unprivileged extension-UI trust zone, capability-scoped messaging, a bounded
compatibility MV3 event-runtime state machine inside
`MAX_NATIVE_VIEW_RESOURCES = 48`, a separate Windows-native admission/resource
policy, storage quotas, and authenticated immutable package activation.
Distribution reuses the blocker stack's generic package-authority model—fixed origins,
authenticated content-addressed packages, monotonic candidate/current/previous
state, staged activation, rollback, and crash recovery—without coupling
extensions to blocker compilation.

The shared meta schema carries a separate bounded native-ownership journal.
Each path-free row binds one `(profile, install, regular/private context)` to
the exact package, active/rollback catalog-set digest, store/grant revisions,
backend target, persistent native incarnation, intent, and conservative phase.
Native backends also bind an optional fixed-width, backend-specific owner
identifier once it is observed. The identifier is immutable for that
incarnation and is required before a native row may claim `NativeOwned`;
identityless `NativeMayOwn` and legacy cleanup rows remain conservative rather
than guessing an owner identity. Compatibility runtimes cannot persist a
platform-native identifier.
A complete-cohort global CAS must commit `NativeMayOwn` before any native call
that could create, retain, or remove an owner. `NativeOwned` is preserved
exactly on disk but treated as may-own after restart; a row clears only after
definite native absence and subordinate resource release. Unknown, corrupt,
duplicate, or over-limit state fails the complete load. The journal survives
profile ancillary degradation/removal and blocks profile deletion until its
rows settle. Session recovery rejects new `Begin` operations and every
acquire-directed `Transition`, while exact release-directed `Transition` and
`Clear` operations remain available to retire existing native owners. The
bounded extension service now owns this persistence and ordering seam. Its
serialized actor joins the Store projection, authenticated repository lease,
native-host authority, profile retirement, and shutdown drain. It refuses clean
worker evidence while a worker-owned runtime or attached authority remains
unresolved or accepted/completed command counts differ. That evidence proves
only worker/resource drain, not durable-journal or native-owner absence.
Production native adapters and product activation remain disabled, so this
coordinator is not a release-enablement claim. Reconstruction also rejects
unreachable clock histories: operation and incarnation high-water
marks are equal, every row binds the same operation/incarnation, phase and row
revision agree, and with `C = high_water - live_rows` plus
`S = sum(live_row_revisions)`, the global revision is within
`1 + S + 3C ..= 1 + S + 7C`. The upper bound includes the one-shot identity
attachment that may be durably fenced before a native call. These internal
consistency checks prevent clock
reuse from a torn or corrupted cohort; they cannot detect a coherent rollback
of the entire database without an external anti-rollback anchor.

The extension repository's materialization schema v3 retains the subordinate
side of that cross-store join. Every durable package pin records the complete
`(profile, install, regular/private context)` owner key, the exact catalog-set
record digest, its historical active/rollback role, the exact package-record
digest, and Store's persistent native incarnation. Repository generation is a
separate transition CAS clock and is never reused as native incarnation.
Store incarnation therefore supplies reopened ABA protection, while a consumed
materialization runtime plus its generation and directory identity protect an
in-process transition. Candidate/current/previous remain the three atomic set
slots; all owner pins retain their complete named set, with at most one
additional owner-only drain set. Recovery validates the package as a row of
that exact set and retains the whole set closure instead of reconstructing a
partial drain from catalog anchors. The durable owner inventory is bounded to
1,024 exact context keys (64 profiles x 8 installs x 2 contexts). That is
crash-recovery capacity, not private-browsing permission: acquisition remains
explicitly regular-only until the private runtime contract is implemented.
The unreleased schema-v2 pin shape lacks these join fields and fails closed.
Cleanup after repository reopen is a separate, access-free path. It accepts
only a Core-minted Store binding for `Release/NativeAbsentReleasePending`,
grants no package or runtime access, and rejects a live lease for the same
owner in the current repository-open epoch. After a classified frontier
preflight proves there is no in-memory or physical object-stage residue, an
absent owner settles before catalog-set lookup so crash-before-pin replay
remains idempotent. A present owner must match the complete persisted
owner/set/role/incarnation identity
and the exact set-row/package/backend join before the existing exact removal
transition may run.
Interrupted package builds settle at a source-free boundary before any new
bundled-resource adapter callback. The package-record final is published last
and is the durable commit marker for its complete object closure. While that
marker is freshly proven absent, recovery may remove only the exact stages
owned by the one durable intent, reprove both marker and stage absence, and
abort the intent. Once the marker is present, abort is permanently forbidden:
the repository rereads its own authenticated catalog object, re-admits the
catalog and stored manifest through product policy, freshly verifies the
index, legal artifact, tree, and package record, and consumes a role-specific
active or rollback completion transition. The public settlement API accepts no
source, and a retry of that same catalog/package/runtime returns after
settlement without consulting its source. Missing, corrupt, or mismatched
post-marker state fails closed and remains unrepairable by the package source.
Every package-record final discovered during recovery must be rooted by either
the exact sole build intent or the completed ledger; unrooted commit markers
are ambiguous durable state, not inert garbage.
The physical recovery inventory currently permits at most eight sealed
catalog-set finals, but materialization does not yet have a production garbage
collector. Repeated distinct selections can therefore exhaust that bound.
Bounded closure GC, ordered after interrupted-build settlement and preserving
every candidate/current/previous and owner-pin root, remains a release blocker;
the current ceiling is not a claim of indefinite operation.
Core package-pin bindings are structural joins, not proof that a Store row is
still current. Fresh acquisition consumes its move-only binding into the live
repository lease; the lease exposes eligibility only by borrow and destroys
the acquisition authority when converted to cleanup-only release. The runtime
service is the exclusive owner of both the Store journal projection and
`ExtensionRepository`: immediately before every repository mutation it
revalidates the exact current row and CAS in its serialized actor turn. Neither
raw bindings nor repository methods cross its bounded mailbox. Internal-only
authenticated authority tests exercise real Store and repository state through
activation, exact retirement, profile retirement, shutdown, and post-drain
Store/repository reopen; stale or caller-synthesized rows cannot reach a
repository transition. These tests use a bounded native fake and do not
substitute for a live platform adapter.

Durable package identity is representation-exact. A bundled authenticated tree
is tagged `BundledTree` and carries no synthetic archive evidence; a future
acquired ZIP is tagged separately and binds both a non-zero bounded byte length
and its SHA-256 before materialization. The profile schema stores the same tag
and nullable/exact ZIP evidence redundantly in install and grant rows, and the
bounded codecs reject any disagreement. Profile schema v12 is a deliberate
fail-closed epoch: the unreleased v9-v11 shape recorded only an archive digest,
so migration preserves the catalog revision and monotonic install-ID floor but
invalidates those inexact install/grant rows. They must be reinstalled through
the exact package authority and their old identities can never be reused.

Permanent ceilings include Manifest V2, persistent backgrounds, blocking
`webRequest` on public WebKit, devtools extensions, browser-identity overrides,
native messaging in the initial target, and an open catalog. Unsupported or
degraded APIs must fail deterministically and be disclosed; they are never
silently approximated. Permanent security invariants and release gates live in
[`security-model.md`](security-model.md); implementation sequencing is not part
of this architecture contract.

---

## 11. Frontend (frame)

Svelte 5 runes + TypeScript + Vite. A pure projection; only transient view
state.

- **One app, entry-routed by surface**: chrome (sidebar/topbar), overlay
  (launcher, palette, find, floating panels), internal pages (history,
  settings, graphs, notes, easel), onboarding. All share components, tokens
  and stores.
- **Styling:** Tailwind v4, token-first. Design tokens are CSS custom
  properties and the single source of truth; runtime theming (Vivaldi-grade
  customization) = swapping variable values, driven by the Themes domain.
  Bits UI for narrowly adopted headless accessible primitives; the look is
  ours. Native HTML and platform-native surfaces remain the default.
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
not that navigation, reload, or rendering completed. Native-runtime update
state uses the stable typed `zephium:runtime-status` event. Its security facts
are a bounded canonical set rather than one lossy highest-priority message, so
UI presentation can evolve without changing the native admission boundary.

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
  tasks/easel, site-as-app windows, blocker source packaging/enablement per §9,
  Tier 1 extensions (Windows), data viz internal pages, html->md and clipboard
  commands.
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
  and blocker preferences, plus per-profile history/favicons with FTS5,
  source-authoritative userscripts, remembered page permissions, and bounded
  structural extension install/grant authority. Those ancillary catalogs are
  durable input only: they do not prove native activation, package
  authentication, or live permission enforcement. The conservative native
  reconciliation journal is separate shared-meta ordering state. It does not
  itself authenticate a package or prove native activation. The implemented
  bounded service coordinator joins that journal to authenticated repository
  authority and lifecycle drains, while production native adapters and product
  activation remain disabled.

---

## Non-negotiables

1. The domain stays pure and `unsafe`-free.
2. No UI surface ever holds authoritative state.
3. All mutations to persistable aggregates go through their reducer.
4. Only `desktop` knows Tauri. Page-derived network access stays inside the
   exact profile-scoped native engine; app-owned downloads require dedicated
   policy components rather than a generic fetch port.
5. Content page worlds are NEVER given a bridge (see security-model.md). Any
   future content-native channel must be isolated-world-only, per principal,
   capability-scoped, natively identity-bound, and disabled until the
   architecture/security documentation and native hostile tests prove it.
6. `unsafe` only in engine/native adapters, each block documented.
7. In-app overlays position relative to the main window, never absolute.
8. A change is not done until `cargo xtask ci` is green on all three targets.
