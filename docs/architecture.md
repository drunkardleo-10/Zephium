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
│   ├── zephium-extension-package bounded manifests, catalogs, tree indexes,
│   │                            portable paths, and CRX3 authentication.
│   ├── zephium-extension-acquisition catalog-bound hostile ZIP preflight and
│   │                            bounded streaming decompression.
│   ├── zephium-extension-authority product-sealed catalog/manifest policy.
│   ├── zephium-extension-repository immutable package materialization, atomic
│   │                            catalog sets, leases, rollback, recovery, GC.
│   ├── zephium-extension-runtime-api move-only native host authority.
│   ├── zephium-extension-service serialized Store/repository/native lifecycle
│   │                            and management transactions.
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
the current tree. The product target is a curated, package-neutral MV3
compatibility surface. Pinned **Bitwarden Core** and stock third-party
artifacts are adversarial acceptance contracts used to expose platform gaps;
they are not product dependencies and no target-specific branch belongs in
ordinary browser code. An open store or general-parity promise remains outside
the initial release.

Delivery is layered and measured:

- declarative data uses native rules/storage and no persistent JS runtime, but
  still has compile, match, and memory cost;
- userscripts/userstyles lazily require a native world/handler registration per
  active principal in each eligible live content controller/view, plus an
  isolated JS context in each eligible frame/document; both cardinalities are
  explicitly capped; and
- the Linux MV3 compatibility subset requires a bounded Zephium-owned
  event-runtime view/context plus any chosen extension-UI or capability-specific
  offscreen resources; WKWebExtension and WebView2-native extension workers are
  engine-managed and require separate count admission and measured
  process-resource gates.

On macOS 15.4 and newer, Apple's public `WKWebExtensionController` stack is the
preferred native MV3 candidate. A feature-gated live probe proves controller
attachment before view construction, explicit host/private-data grants,
per-extension isolated worlds, frame matching, exact context unload/reload,
preservation of Zephium's protected scripts, MV3 background execution, and
same-principal extension-storage isolation across two persistent controller
namespaces and fresh controller instances. A second behavioral gate constructs
two regular profiles through the exact product registry and one
nonpersistent private session. It pointer-attests each view/controller/store
binding, proves mutually exclusive cookie and extension-storage state, proves
regular reconstruction and private noninheritance, and publishes distinct
tab/window/delegate graphs to demonstrate that each controller exposes only its
own profile surface. The probe retires each namespace independently, reopens
both to verify zero persistent extension bytes, and requires every native
view/controller/context/store and routing object weak reference to release.

The delegate always enumerates the complete logical tab set, including
discarded tabs, but it returns no `WKWebView` for a non-resident tab. That
callback cannot carry a typed error, so the refusal increments one saturating,
URL-free per-profile diagnostic instead of inventing a generic mutation
failure. The native gate verifies that the physical-view resolver is never
called for the discarded tab and that the diagnostic advances. Extension
enumeration therefore cannot resurrect a renderer or defeat §7's memory
policy.

Extension-driven tab reload and back/forward traversal use the same bounded
native-completion broker as create, activate, navigate, and close. Shell owns
the mutation decision and dispatches only to an already-resident tab in the
focused profile scope; a discarded or in-flight-discard tab receives a typed
`TabDiscarded` refusal and is never recreated. History traversal also requires
the latest Shell-owned native history flag. Cache-bypassing reload remains an
explicit refusal because the cross-platform Engine contract currently exposes
ordinary reload only; approximating it would report semantics the browser did
not apply.

Native grant replacement is an exact, bounded main-thread operation. The pure
grant compiler supplies the complete allowed API/host set; the adapter clears
all four WebKit permission dictionaries, revokes every bounded prior key
through the per-key status API, applies every new key through that same API,
and accepts the generation only after exact dictionary/status/private-access
readback. This distinction is behavioral: bulk dictionary assignment alone
does not recompute content-script eligibility for a loaded context. The live
probe therefore proves host-grant activation, revocation, and restoration on
subsequent navigations, then unloads the context before clearing and verifying
final absence. The production macOS adapter reaches this boundary only through
authenticated package, grant, ownership-journal, and operation authority
reconstructed inside the serialized extension service.

The same identity rule applies to runtime `permissions.request()`. Grant
revision and digest are inputs to the runtime fingerprint, durable native
ownership row, native grant snapshot, and operation authority. A delegate may
therefore never persist a grant and patch only `WKWebExtensionContext`, nor may
it return an allowed set before those authorities agree. Runtime optional
permission requests remain denied until the serialized coordinator implements
a crash-consistent upgrade protocol. The manual native gate now proves the
platform settlement semantics with genuine clicks in an extension-origin page:
WebKit invokes the API-permission and host-pattern callbacks separately with no
tab for this context-scoped request; complete denial leaves all four native
grant/denial dictionaries empty; partial approval of a combined API-plus-host
request settles JavaScript as denied and retains neither subset; and complete
approval settles JavaScript as allowed and retains exactly both grants. The
delegate must therefore collect and authorize the complete request without
depending on callback order or tab presence. The gate runs explicitly with:

```sh
cargo run --locked -p zephium-engine --features native-web-extension-probes --bin macos-web-extension-probe -- --interactive-permission-gate --require-supported-runtime
```

Normal unattended CI compiles this path but reports it as
`interactive-not-run` because WebKit rejects `permissions.request()` without a
trusted user gesture. The upgrade protocol must either atomically supersede
every fingerprint-
bound authority around a verified complete live native replacement, or retire
the old owner before persistence and prove that WebKit can truthfully settle
the originating request across replacement. Live context grant replacement
remains useful for native behavioral verification, but is not durable product
authority by itself.

The controller registry now separates persistent namespace identity from
quiescence. View construction and exact controller borrowing revalidate the
profile identifier, persistent controller configuration, and the exact
`WKWebsiteDataStore` pointer while allowing that controller to contain active
contexts. Creation, profile erasure, and clean shutdown additionally require
the controller to contain no contexts or extensions. Controller preparation is
not a startup side effect: the native lifecycle must supply the exact durable
`MacosControllerV1` namespace scope, preparation is bounded to the profile
ceiling and idempotently reuses an existing entry, and a borrow never creates a
missing namespace.

The native activation boundary constructs one move-only macOS owner from a
provider-validated package-root lease, a complete compiled grant snapshot, an
exact catalog-authenticated 32-byte Chromium identifier, and the retained
profile controller. The root path is borrowed exactly once; only a byte-for-byte
read-back file URL crosses the callback boundary, while the package lease stays
retained by the host reservation. Activation accepts only MV3 with zero parse
errors, assigns and re-reads the exact native identifier, applies the complete
grant set, and pointer-attests the context, extension, and controller after
load. Teardown unloads the exact context, clears every bounded native key seen
at cleanup (not merely keys from the last plan), and mints the macOS absence
audit only after controller-membership and unloaded-state readback. A native
exception while entering asynchronous parsing is ownership-uncertain, never a
never-entered rejection. The ordinary macOS factory joins activation,
retirement, same-process reconciliation, restart reconciliation, and shutdown
drain as one recoverable lifecycle. Startup first settles cleanup, then loads a
bounded canonical inventory of enabled regular runtimes and replays the same
authority transaction before Shell may construct any profile webview. Hydration
retries resume monotonically without re-entering cleanup around a runtime
already acquired by the same worker; strict runtime-capacity deferrals and
exact rejections are retained and counted in readiness evidence.

An extension-origin page is not navigated in a normal profile view. WebKit
requires the loaded context's customized
[`webViewConfiguration`](https://developer.apple.com/documentation/webkit/wkwebextensioncontext/webviewconfiguration),
so toolbar popups use WebKit's native action popover and a separately budgeted
popup view rather than navigating, replacing, or overlaying a content tab. One
private extension context per installed extension lives for the private-session
lifetime; its UI views may be recreated from that context, while destroying the
session context, controller, and nonpersistent store is the storage-erasure
boundary.

The macOS Bitwarden contract gate additionally resolves its tab-specific
`WKWebExtensionAction`, verifies label/enabled/popup state, and opens the
declared popup `WKWebView`. That view must point to the exact extension
controller and profile `WKWebsiteDataStore`; the gate proves load,
`closePopup`, reopen, and final wrapper release when the context retires.
The contract runs on a persistent regular profile prepared through the product
controller registry, not on a nonpersistent stand-in: Wry's constructed view
must pass the normal controller/store attachment attestation, the Shell-shaped
browser surface supplies the existing resident tab, and teardown closes that
surface before unloading the context, clearing grants, erasing the exact probe
principal's data record, and releasing the registry. This distinction is
behavioral. WebKit treats a nonpersistent website data store as private even
when a test delegate labels its logical window regular, which suppresses
content injection without the independent private-data grant.
WebKit may cache the wrapper and its last URL after `closePopup`, so production
holds the process-wide `ExtensionPopup` resource lease only for the presented
interval and treats the documented close call—not wrapper deallocation—as the
presentation-resource release boundary. Only one such lease exists inside the
same hard 48-resource ceiling.

Toolbar projection is a replaceable Shell-owned cohort keyed by the exact
profile, logical tab, browser-surface generation, runtime generation, and
monotonic native action revision. The engine enumerates only operation-authority
published runtimes in stable native `Owned` state, resolves the already-created
logical `WKWebExtensionTab`, and calls `actionForTab:` without reading the tab's
weak webview or accessing `popupWebView`. Labels and badges cross the boundary
only after fixed byte validation. WebKit decodes the declared icon natively;
the adapter exception-contains and rasterizes its `NSImage` into one exact
32x32 straight-alpha RGBA buffer, so privileged chrome never decodes an
extension-controlled image format or retains a native image graph. Unchanged
effective state reuses its prior revision. An applied empty cohort removes stale
buttons, while a rejected or stale refresh retains the last known-good cohort.
`didUpdateAction` is accepted only from the exact profile controller, loaded
context, and (when present) known logical tab, then reduced to one coalescible
profile invalidation fact. Scheduled reads remain in a fixed-size pending set
until an exact applied settlement arrives. The existing low-frequency
maintenance tick rereads each active profile to repair a rejected or lost
settlement/invalidation without adding a timer, renderer, or idle wakeup.
Profile retirement erases both the logical surface and its action cohort. This
read path is not invocation authority. A toolbar invocation is independently
versioned by request id, surface generation, runtime generation, and action
revision; Shell derives the current tab rather than accepting one from IPC.
The native host revalidates residency and action state, optionally joins a
declared `activeTab` witness to the exact currently-presented HTTP(S) document,
then revalidates again before `performActionForTab:`. Missing `activeTab` or a
restricted document never suppresses the separate click event, while capacity
or identity contradictions fail closed. Non-popup actions are dispatched;
popup actions first reserve the one process-wide `ExtensionPopup` lease and an
exact profile/controller/context/tab callback expectation. A programmatic or
mismatched WebKit callback is rejected. A matching loaded popup is shown as a
transient native `NSPopover` relative to the privileged Shell anchor, with
finite clipped anchor geometry, continuously clamped 64x48--800x600 content,
outside-click/Escape dismissal, a ten-second load watchdog, and exact-once
terminal settlement. Surface replacement closes a popup when its tab becomes
discarded, inactive, or absent; runtime retirement, profile erasure, and host
shutdown cancel loading and close the native popup before owner release. The
lease covers only the pending/presented interval and is released even though
WebKit may cache its popup wrapper.

The privileged frame receives an actor-revisioned exact-replacement action
cohort only for the focused profile and active logical tab. Fixed RGBA icons
use canonical base64 rather than a 4,096-element JSON integer array. The frame
joins the projected profile/tab to the current Items snapshot, rejects delayed
action or failure evals against the same monotonic revision floor, and echoes
only install/runtime/action revisions plus the clicked button's CSS viewport
rectangle. Desktop translates that rectangle through the generation-checked
chrome origin into window-logical coordinates, then accepts canonical ULIDs,
fixed lowercase nonzero hex, a fully visible finite anchor, the main-window
caller, and a live shutdown state;
Shell and native code still derive and revalidate every authorizing fact.
Synchronous and asynchronous refusal map to a closed, profile/tab-bound
user-visible taxonomy without exposing native strings, URLs, extension content,
or runtime identity. A surface-generation replacement emits an empty cohort
before its asynchronous native refresh, so even a same-tab reconstruction
cannot preserve a stale button.
Toolbar rendering is allocation-bounded by the eight-install profile ceiling;
disabled actions remain visible but inert, badges are clipped, and no idle
animation, renderer, timer, or popup resource exists until an actual failure or
trusted click.

The repository/service tests prove authenticated package admission and
startup/crash reconciliation against a bounded native fake, while the live
WebKit probe proves the platform adapter. The authenticated product probe now
joins package authority, native ownership, controller activation, the
Shell-owned window/tab delegate, and real content-script execution. Production
provisioning remains deliberately empty; ordinary release builds therefore
expose no extension runtime. Authenticated catalog management and install UX,
native action projection, and transient popup hosting are implemented; the
install review always includes required API/host authority, defaults every
optional declaration to denied, and can return only bounded indexes into the
exact canonical candidate retained by Shell. The serialized service
reauthenticates that package, resolves the selected declarations from its
manifest, and atomically initializes the grant cohort; local-file and private
browsing access remain separate explicit decisions. Reviewed degraded
declarations also flow from that exact admitted manifest into a bounded typed
disclosure cohort. API names are revalidated tokens; structural declarations
collapse into browser-owned feature categories; compatible rows cannot carry
limitations and degraded rows cannot omit them. The install review renders the
complete cohort before consent, while installed rows show a bounded summary;
neither surface receives native error strings or invents compatibility from a
package name. Dynamic optional-grant requests are still denied by the native
default and have no user prompt. The
remaining release gaps include that runtime permission broker, the complete
permission and API matrix, quotas, release-build resource evidence, and
endurance. Older admitted
macOS versions and Linux require a Zephium
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
The move-only application lifecycle is also the only app-facing activation and
exact-runtime retirement ingress. Inputs are identity selectors, not authority;
successful settlements carry the complete active-profile routing cohort from
the same serialized worker turn. Shell therefore never guesses whether a
sibling runtime still keeps a profile active, and cloneable status handles
remain observation-only.
The macOS native adapter is enabled, but sealed product provisioning remains
empty and the Windows/Linux adapters remain unavailable, so this coordinator
is not a release-enablement claim. Reconstruction also rejects
unreachable clock histories: operation and incarnation high-water
marks are equal, every row binds the same operation/incarnation, phase and row
revision agree, and with `C = high_water - live_rows` plus
`S = sum(live_row_revisions)`, the global revision is within
`1 + S + 3C ..= 1 + S + 7C`. The upper bound includes the one-shot identity
attachment that may be durably fenced before a native call. These internal
consistency checks prevent clock
reuse from a torn or corrupted cohort; they cannot detect a coherent rollback
of the entire database without an external anti-rollback anchor.

The product-sealed catalog authority now distinguishes payload origin at the
capability boundary. `AdmittedBundledCatalog` can contain only `BundledTree`
rows; `AdmittedAcquiredCatalog` can contain only exact `AcquiredZip` rows and
requires every row to carry a complete expected Chromium key identity. Both
paths authenticate the same exact active catalog anchor and product-owned
license policy, but their non-cloneable witnesses are nominally distinct and
cannot cross into the wrong materializer. Acquired manifest admission has the
same separate typed ingress. This adds no catalog, download, or startup work to
ordinary builds: production sealed provisioning remains empty, arbitrary
caller-parsed catalogs remain non-authoritative, and an acquired witness grants
no network, archive, repository, profile, or native-runtime authority.

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

`zephium-extension-acquisition` owns the first acquired-package boundary and is
not linked into the ordinary inert product path by any runtime crate. It accepts
only CRX3 bytes whose signed developer key derives the product-expected
Chromium id and whose inner ZIP byte length and SHA-256 exactly match an
`AcquiredZip` catalog identity. The production-shaped release-row constructor
also requires the complete 256-bit CRX developer-key digest from the row's
Chromium identity; the shorter derived extension id is not treated as complete
key authority. Before constructing the ZIP parser, an
allocation-free terminal-record preflight bounds the entry count and central
directory. Complete preflight then admits only stored or deflated ordinary
files/directories, canonical ASCII portable paths, one root `manifest.json`,
and the existing per-file, aggregate-tree, entry, depth, and retained-memory
ceilings. It rejects ZIP64/multi-disk framing, encryption, links and special
files, duplicate/case/device/file-directory aliases, ambiguous local headers,
and overlapping payload regions. Files can leave the boundary only through a
bounded streaming copy that reaches EOF and therefore checks decompression,
exact length, and ZIP CRC; the boundary opens no paths itself. Every successful
copy returns one non-cloneable receipt containing the digest of the exact byte
prefix the destination writer accepted. An exact, duplicate-free receipt cohort
derives canonical tree-index bytes, reparses them through the shared bounded
canonical index boundary, and retains both forms under one named memory
ceiling. Binding that completed tree to a release row rechecks the acquired
payload, full CRX key digest, derived id, manifest digest, tree digest, index
digest/length, file count, and aggregate bytes.

The offline CRX probe materializer consumes this same implementation and
completes the stream receipt after per-file synchronization, so diagnostic and
product definitions of safe extraction cannot drift. The repository's opt-in
`acquired-packages` boundary now streams each authenticated archive file once
directly into a create-new private tree, retains only the bounded root
manifest, binds the complete receipt cohort to the release row, bottom-up seals
the stage, and then independently enumerates and re-hashes every closed file.
No stream callback or receipt alone is publication authority, and the ordinary
product graph does not link the archive stack. The prepublication tree uses one
canonical `.acquiring` name distinct from both content objects and durable
build stages. It is never a state or journal root: after proving both repository
controls exist, open drops its recovered capability and removes that exact
bounded tree before state recovery. Missing controls, multiple acquisition
stages, or an object/stage digest alias fail closed before cleanup. A crash may
therefore discard work but can never resume partial bytes as authority or
manufacture a pristine repository. The remaining product transaction must
admit the acquired manifest, persist the canonical index and legal artifact,
and publish the package record last before install or activation.

Permanent ceilings include Manifest V2, persistent backgrounds, blocking
`webRequest` on public WebKit, devtools extensions, browser-identity overrides,
native messaging in the initial target, and an open catalog. Unsupported or
degraded APIs must fail deterministically and be disclosed; they are never
silently approximated. Permanent security invariants and release gates live in
[`security-model.md`](security-model.md); implementation sequencing is not part
of this architecture contract.

The pinned Bitwarden Core `browser-v2026.7.0` contract treats `webRequest` as
required. Its [exact background implementation](https://github.com/bitwarden/clients/blob/browser-v2026.7.0/apps/browser/src/autofill/background/web-request.background.ts)
uses that namespace only for HTTP Basic-auth autofill: `onAuthRequired` is
registered with `asyncBlocking`, while completion/error observation only
retires pending request identities. The macOS live gate now proves that WebKit
parses the permission, exposes `chrome.webRequest`, starts the MV3 background
worker, accepts that exact listener-registration shape, and invokes the
non-blocking `onCompleted` listener after the already-registered worker observes
a real HTTP navigation in an existing regular product tab. The content script
must also run in that document before the callback observation is accepted.
This proves observation, not blocking semantics: WebKit does not provide the
blocking behavior the Basic-auth workflow requires, and the upstream
`webRequestAuthProvider` declaration is filtered by the native parser. The
reviewed macOS classification is therefore:

| Bitwarden surface | macOS native classification | Evidence / boundary |
|---|---|---|
| MV3 background startup with declared `webRequest` | Compatible | Persistent product-profile live background registration gate |
| Toolbar action and declared popup page | Native-brokered | Exact action/icon projection, privileged gesture admission, native transient popup presentation, bounded failure UX, resource admission, and teardown are implemented; release-build end-to-end UX/RSS/endurance evidence remains required |
| Non-blocking request observation | Compatible on the exercised runtime | Exact upstream-shaped `onCompleted` registration observes a real regular-tab HTTP navigation; the supported macOS floor still requires the same release-runner evidence |
| HTTP Basic-auth autofill | Degraded | Startup survives, but the required blocking callback semantics are unavailable |
| Network blocking/modification through `webRequest` | Unsupported | Zephium never emulates synchronous request control through a generic bridge |

The same product-shaped background gate records a closed runtime-namespace
inventory, not just the permissions WebKit retained while parsing. On the
current native gate WebKit exposes `alarms`, `commands`, `contextMenus`,
`scripting`, `storage.local`, `tabs`, `webNavigation`, and `webRequest`, while
`idle`, `notifications`, `offscreen`, `sidePanel`, and `storage.managed` are
absent. A namespace being present is not a behavioral compatibility claim;
each API that affects the pinned workflow still needs an exact live gate. The
product-tab gate additionally proves a main-world file injection when the
literal `"MAIN"` value is supplied, a top-level `webNavigation.onCommitted`
callback, execution through an opaque dynamic web-accessible-resource URL, and
the candidate sealed-Blob sandbox boundary. It also verifies alarm create/read/clear,
exact manifest-command enumeration plus native-to-background dispatch, and
context-menu create/update/native projection/remove.
WebKit omits the `ExecutionWorld` enum even though literal `"MAIN"` main-world
injection succeeds. The typed runtime classification is therefore
`literal-main-only`, not main-world unavailability; a pinned extension that
dereferences the absent enum still needs an exact reviewed source adaptation.
A manifest-declared sandbox page,
including one placed in an explicit `sandbox="allow-scripts"` iframe, retains
both a `webkit-extension` origin and the `chrome` API. The viable adapter gate
therefore removes the leaf HTML and privileged intermediate page from public
resources, exposes only an inert payload, and lets the authenticated content
script create a Blob-backed `allow-scripts` frame. The live gate observes a
`null` message origin, no `chrome`, `runtime`, or `storage.local`, and no parent
DOM access; direct leaf exposure is absent. This proves the platform primitive,
not the still-unimplemented pinned-source transform. The release runner at the supported
OS floor must reproduce the closed inventory and these exact behavioral
classifications before they become floor-wide claims.

| Bitwarden surface | macOS native classification | Evidence / boundary |
|---|---|---|
| MV3 backup-localStorage through `offscreen` | Requires a reviewed Bitwarden Core adapter before release | The pinned entrypoint always selects [`OffscreenStorageService`](https://github.com/bitwarden/clients/blob/browser-v2026.7.0/apps/browser/src/platform/storage/offscreen-storage.service.ts) for MV3. Primary writes survive because the upstream [`PrimarySecondaryStorageService`](https://github.com/bitwarden/clients/blob/browser-v2026.7.0/libs/common/src/platform/storage/primary-secondary-storage.service.ts) settles both writes, but a primary miss reaches the absent API. Zephium must select a deterministic primary-only/recovery-compatible adapter in its sealed build and test migration, missing-key, and recovery behavior; it must not inject a page-world shim. |
| Idle and system-lock integration | Degraded | The pinned [`IdleBackground`](https://github.com/bitwarden/clients/blob/browser-v2026.7.0/apps/browser/src/background/idle.background.ts) returns immediately when the namespace is absent. System-lock vault timeout and idle-driven notification reconnect/disconnect are unavailable; ordinary timer-based vault locking remains a separate behavioral gate. |
| System notifications | Degraded | The pinned composition selects `UnsupportedSystemNotificationsService` when `chrome.notifications` is absent. In-extension auth-request flows may remain available, but OS notification presentation/click handling is unavailable. |
| Chrome side panel | Degraded | The pinned [`BrowserApi`](https://github.com/bitwarden/clients/blob/browser-v2026.7.0/apps/browser/src/platform/browser/browser-api.ts) capability-checks the namespace and makes side-panel operations no-ops. Zephium's native toolbar popup remains the primary extension UI. |
| Enterprise managed storage | Unsupported in the initial target | `storage.managed` is absent, so enterprise policy supplied through that browser API is not exposed. This does not authorize approximating managed policy with writable extension storage. |
| Native messaging | Unsupported in the initial target | The optional permission is parsed, but product grant compilation prohibits it. Native biometric/application integration must be disclosed separately from core vault use. |
| Programmatic main-world scripting | Native literal supported; pinned source requires a sealed adapter | `scripting.executeScript` injects the exact extension file into the product tab when passed literal `"MAIN"`, but WebKit exposes no `chrome.scripting.ExecutionWorld` enum. The sealed build must substitute the absent enum access without adding page-world privilege or a generic bridge. |
| Runtime ports | Registered routing compatible; pre-listener connection not queued | A Zephium-owned extension page opens a named port only after the MV3 worker registers `runtime.onConnect`; the worker receives the port and completes an exact message round trip. A separate connection created immediately before listener registration returns a port, then disconnects without `runtime.lastError` and is not delivered after registration. Extensions that depend on Chrome queuing that startup race require a reviewed compatibility decision; ordinary registered port routing does not. |
| Non-blocking top-level navigation observation | Compatible on the exercised runtime | A background `webNavigation.onCommitted` listener observes the real regular product-tab HTTP navigation. Frame/detail behavior remains outside this gate. |
| Dynamic web-accessible resources | Compatible on the exercised runtime | A content script resolves an opaque runtime URL for a `use_dynamic_url` resource; the page loads and executes the declared resource. Revocation, multi-frame behavior, and hostile embedding remain separate gates. |
| Manifest sandbox pages | Sealed adapter primitive compatible; pinned transform still required | WebKit does not enforce the declared sandbox and also ignores the intended isolation when the extension URL is placed in an explicit sandboxed iframe. The live gate proves the reviewed replacement topology: an inert public payload is fetched by the authenticated content-script host and instantiated through a Blob-backed `allow-scripts` frame; the child has a `null` message origin, no extension APIs, and no parent DOM authority, while the privileged leaf URL is not public. The exact Bitwarden button/list bundle transformation and authenticated message flow still require deterministic source/build adaptation and hostile end-to-end tests. |
| Private extension-resource WebAssembly startup | Release-blocked on the exercised runtime | The pinned 7,378,704-byte SDK module is returned as `application/octet-stream`. Native streaming compilation rejects that MIME, the vendor `arrayBuffer()` fallback promise does not settle, and the popup event loop stops advancing before compilation starts. A separately labelled probe that wraps the same private response body with `Content-Type: application/wasm` in both popup and background also stalls during the streaming retry. This closes a header-only runtime workaround; it does not authorize embedding bytes, adding a generic resource bridge, or distributing a modified package. |
| Alarms | Lifecycle admission compatible; delivery unassessed | The background creates an exact future alarm, reads back its name and finite scheduled time, clears it, and proves post-clear absence. Firing, service-worker wakeup, restart persistence, sleep/wake behavior, and long-duration drift remain release gates. |
| Commands | Native dispatch compatible; keyboard routing unassessed | `commands.getAll` returns the exact six pinned command names. The host selects the native `autofill_login` command from the profile context, verifies its context identity, performs it through `WKWebExtensionContext`, and the background receives the exact `commands.onCommand` identifier. Physical shortcut event matching, collision handling, remapping, focus behavior, and `_execute_action` routing remain release gates. |
| Context menus | Native projection compatible; click routing unassessed | The background creates and updates one tab-context item; `menuItemsForTab` returns exactly that updated native item for the product tab; the background then removes it and the probe settles only after removal. Product-tab menu presentation at a pointer location, click routing, frame/editable context, enablement updates, and teardown under navigation remain release gates. |
| Clipboard read/write | Behavior unassessed | A production gate must be driven by a trusted popup gesture and preserve the user's prior clipboard contents on every success, refusal, timeout, and crash path. Automated tests must not destructively overwrite ambient clipboard state. |

This is one reviewed slice of the required declaration-by-declaration matrix,
not a claim that the complete Bitwarden workflow is compatible. Product
authority remains unprovisioned until every declared authority is classified
and the exact sealed package/catalog digests are compiled in.

The release-side source boundary is offline and separate from package
authority. `cargo xtask check-bitwarden-core-source --source PATH` accepts only
an ordinary, non-sparse, clean checkout whose `HEAD` is the exact
`browser-v2026.7.0` commit and tag. It additionally hashes every reviewed
compatibility preimage and checks exact marker cardinality for the offscreen,
main-world, manifest, inline-menu, template, and webpack seams. The command
does not clone, fetch, adapt, build, or authorize a package. Git output and
individual source reads are bounded; a partial inspection checkout, local
edit, untracked input, symlink escape, or source drift fails before adaptation.
The native vertical slice builds the OSS production **Chrome MV3** target. The
pinned Safari manifest transform deletes the service-worker `background`
declaration and is therefore not the input to Zephium's WKWebExtension
controller runtime. This target choice is compiled into generated overlay
metadata rather than left to a release-shell environment default.

`cargo xtask materialize-bitwarden-core-macos-probe-overlay --source PATH
--output PATH` emits an atomic, no-replace source overlay only after that exact
admission succeeds. It substitutes literal `"MAIN"`, selects primary storage
when `chrome.offscreen` is absent, classifies the unbranded WK service-worker
environment as Safari-extension shaped, guards the absent notification
subscription surface, and fail-closes the unsafe inline-menu path while removing
its sandbox and public resource declarations. Popup stage markers are explicitly
probe-only. The metadata states `product_authority=false` and
`inline-menu-disabled`; this overlay exists to run the first real native vertical
slice and cannot be sealed as the release package. The production overlay must
replace that degradation with the self-contained inert-payload renderer proven
by the live native gate.

`cargo xtask finalize-bitwarden-core-macos-probe-artifact --build PATH --output
PATH` is the next offline boundary, not a release sealer. It accepts only the
exact reviewed Chrome-MV3 output shape, rejects links and special files, removes
exactly nine source maps and nine disabled inline-menu outputs, injects four
explicit probe resources, and atomically emits a closed 173-file extension tree.
Every retained byte is bound into the same canonical path/length/SHA-256 tree
format used by package authority. The sibling metadata binds the source commit,
target, manifest, tree/index hashes, adaptation inventory, stripped counts, and
the negative claims `product_authority=false` and
`build_toolchain_attested=false`.

The debug-only `macos-bitwarden-core-probe` independently reopens that artifact,
checks its closed root inventory, reparses the canonical index, and re-hashes
every extension file before invoking WebKit. It then exercises exact native
grants, real content registration, action-owned background/popup startup, an
extension-page canary, and full native teardown. As of the August 12, 2026 gate,
the exact build reaches registered content and a real Bitwarden popup with every
static/dynamic script loaded, but fails the 12-second executable-popup deadline
while importing the 7.04 MiB SDK WebAssembly module. WebKit serves the private
extension resource with a MIME type that rejects streaming compilation; the
vendor webpack fallback is allowed to run, but does not settle within the UX
deadline on the reviewed machine. Bounded phase evidence on August 14 sharpens
that result: the 7,378,704-byte resource returns status 200 and
`application/octet-stream`; streaming rejects immediately, `arrayBuffer()`
returns its promise immediately, that promise never resolves, and
`WebAssembly.instantiate` is never called. A 100 ms probe timer also stops near
188 ms, so the failure is resource-body/event-loop progress rather than slow
compilation. Three uncontended release-build repetitions
on August 13, 2026 reproduced the same `sdk-import` / `streaming-rejected`
terminal state in 12.92–13.32 seconds, at 115–119 MB maximum resident set and
approximately 110.5–111.0 MB peak footprint for the probe process. The blocker
is therefore stable on the exercised machine rather than a contention
artifact. It is a release blocker and resource-budget input, not authority to
increase the timeout, preload a hidden view, install a global fetch shim, or
claim Bitwarden compatibility.

The separate feature-gated `macos-web-extension-resource-probe` closes the
public `baseURL` plus `WKURLSchemeHandler` hypothesis with a Zephium-owned MV3
fixture. An ordinary nonpersistent `WKWebView` routes its HTML, JavaScript, and
empty WASM module through the exact native handler, preserves
`application/wasm`, and passes `WebAssembly.instantiateStreaming`. A loaded
`WKWebExtensionContext` accepts and reads back the custom principal-bound base
URL, retains its extension runtime identity, and returns a configuration to
which the same handler is attached. Nevertheless, its private extension
resources never dispatch to that handler: WebKit's internal extension loader
serves the WASM as `application/octet-stream`, and streaming compilation is
rejected. The gate requires zero extension-handler callbacks, denies a foreign
principal, and proves controller, context, view, data-store, and handler
teardown. This rules out that public resource override on the exercised runtime;
it does not claim that future WebKit releases cannot add a supported transport.
The separately labelled response-MIME diagnostic also retries native streaming
with the original private body wrapped in a new `Response` carrying
`application/wasm`. With that adaptation active in both the popup and MV3
background, the runtime reaches `streaming-mime-retry` near 178 ms but neither
the retry nor its timer advances before the same 12-second deadline; native
extension objects also remain retained on that error path. The artifact records
the adapter in both its compatibility and limitation contracts, remains
`product_authority=false`, and cannot be confused with the baseline artifact.
This closes a header-only response adaptation as well as the public scheme
handler route. The stock Bitwarden deadline therefore remains blocked unless
upstream output changes, WebKit changes its private loader, or a separately
approved sealed adaptation removes the dependency on that resource behavior.

The package-neutral Chrome acquisition boundary now authenticates CRX3 before
materialization. It bounds the package and protobuf header, verifies every
recognized RSA/ECDSA proof, requires the signed developer proof to derive the
declared and expected Chrome identifier, and exposes only the authenticated ZIP
payload. The offline probe materializer then preflights every archive entry,
rejects encryption, links, special files, nonportable paths, cross-platform
collisions, file/directory conflicts, duplicate entries, and decompression
budget violations before writing into a private, incomplete-marked,
no-replace tree. It now completes the canonical stream receipt, but that receipt
still does not prove a filesystem snapshot: catalog release authority, staged
tree re-verification, atomic repository publication, and a live lease remain
separate requirements.

The second stock-manager gate uses the unmodified Chrome Web Store CRX for
[Proton Pass 1.38.2](https://chromewebstore.google.com/detail/proton-pass-free-password/ghmbeldphafepmbegfdlkpapadhbakde)
(`ghmbeldphafepmbegfdlkpapadhbakde`). Its authenticated CRX
SHA-256 is
`2cc54d72218fe09081e8e38261ae0f16aca51c62722828f46a33109f88c7c679`;
the exact 275-file, 20,124,326-byte tree is fixed by the probe contract. WebKit
parses and loads the manifest, creates the context and action popover, and
executes enough stock popup JavaScript to populate the application root.
However, it reports `WKWebExtensionContextErrorDomain` code 6 (background
content failed to load), the stock content scripts produce no login-field or
inline-root effect, and the popup does not establish a usable runtime contract
under the probe. Controller, context, page, popup, and routing objects release
after unload, but the nonpersistent website data store remains retained beyond
the five-second error-path teardown budget; repeated failing activation must
therefore stay disabled until that lifecycle is bounded. This proves that this
raw stock Chrome package is not usable on the exercised WKWebExtension runtime.
It does **not** prove that Safari-authored packages or a reviewed
package-neutral compatibility transform are impossible.

A separate authenticated-tree diagnostic now identifies two narrow WebKit API
compatibility requirements without changing production authority. It copies
only the indexed Proton tree into a private no-replace stage, re-hashes every
source while copying, and adds a Zephium-owned prelude and background wrapper.
The prelude preserves native `chrome` and `browser` identities, creates only a
missing alias, locks both global properties around the same native objects, and
provides an inert `runtime.onUpdateAvailable` event because Zephium's
authenticated catalog owns updates. That diagnostic imports the real
background, executes the isolated content-script prelude, renders the popup,
and releases every native object.
It records `offscreen` as absent and `user_workflows=unassessed`; it neither
proves Proton login/autofill compatibility nor authorizes a package. The result
shows that the stock failure includes a missing catalog-update event and an
unstable namespace-global seam, rather than requiring a Proton-specific product
runtime.

Both third-party contracts stay feature-gated and confer
`product_authority=false`. Zephium must not hardcode either manager into the
production runtime or redistribute modified third-party bytes by accident.

The first package-neutral macOS transform now exists as an offline,
non-authorizing boundary. `cargo xtask
materialize-macos-extension-compatibility --extension PATH --tree-index PATH
--output PATH` reopens an exact closed MV3 tree, rejects source drift and
reserved-namespace collisions, and emits a separately indexed artifact. Its
versioned WebKit adapter preserves and locks native namespace identities,
supplies only the inert catalog-update event owned by Zephium, wraps classic or
module background workers, prepends isolated content scripts, leaves `MAIN`
scripts unchanged, and inserts the local prelude only into an explicit leading
action-popup `<head>`. It does not proxy extension APIs: live WebKit evidence
showed that proxy replacement accepts listener registration but breaks native
message delivery. Ambiguous HTML, nonportable or absent resources, links,
special files, unsupported worlds, and every file/tree budget violation fail
closed.
The emitted metadata binds source and output manifest/tree/index identities and
states `product_authority=false`; it is not a release sealer, catalog entry, or
redistribution decision. A Node contract gate exercises native identity,
locked-global and alias behavior, idempotence, unadaptable-native fail-closure,
and the deliberate absence of page/network bridges.

The exact Proton source transformed through that generic boundary produces a
277-file, 20,126,819-byte tree with tree SHA-256
`92bc016384e69ab91a1166b3c84a1c6ac0cd9aaf0d83bdbff6949c6f49b5a810`.
The feature-gated native probe independently pins the source and output
identities before WebKit: the original background loads without context errors,
the real popup observes the package-neutral compatibility world and renders,
and every native object releases. The synthetic login page still shows no
Proton inline-autofill effect, and WebKit exposes no controller-visible content
script in that run. Content-script behavior, login, vault, save, autofill, and
user workflows therefore remain unassessed; this result proves a reusable
popup adaptation seam, not Proton compatibility.

A separately indexed, Zephium-owned MV3 fixture now gates the package-neutral
path without third-party bytes. `cargo xtask ci` materializes its exact source
tree through the same offline transform and executes the result in public
WKWebExtension APIs. On the reviewed macOS 26.6.1 runtime, native namespace
identity remained intact and locked in both the isolated content world and a
module background worker; a bounded background readiness barrier completed in
88 ms; `runtime.sendMessage` returned a synchronous response; a separate
`tabs.sendMessage` reached the content listener; the native sender tab was
present; the page world could not observe the adapter marker; and controller,
context, page, data store, and routing objects all released. The barrier uses
Apple's public
[`loadBackgroundContent`](https://developer.apple.com/documentation/webkit/wkwebextensioncontext/loadbackgroundcontent%28completionhandler%3A%29)
completion contract and does not retain a hidden WebView or prevent ordinary
MV3 suspension. This closes the transformed content/background execution gate;
real stock login, vault, save, and autofill workflows remain the next product
evidence before any package is presented as installable.

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
