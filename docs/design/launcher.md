# Launcher and shared floating tools

The existing `panel` window is the single floating host. This change adds no
window or WebView. Search remains backed by the browser actor; Notes, Tasks,
AI Chat, History, Downloads, and Time tool views are session-only layouts.
Tool previews do not implement their corresponding services. Horizontal tabs
are a later consumer of the placement interface.

## Ownership and routing

`desktop/src/overlay.rs` owns presentation, focus reconciliation, geometry, and
operation settlement. Its pure `overlay/model.rs` separates publication revision
from search session identity. Rust publishes `PanelState`; the frontend sends
bounded `PanelIntent` values and never positions the window itself.

The shortcut opens search from hidden, hides visible search, and returns a tool
to search. Back returns to search; close hides. Search loses presentation when
focus leaves Zephium or moves to its browser. An owned native surface does not
count as another application. Tools stay available within Zephium, suppress
while another application is active, and return unless explicitly dismissed.
Native focus observations are coalesced before reconciliation.

The existing caller policy limits panel ready, drag, and search commands to the
panel. Main and panel can issue bounded route intents; neither grants page
WebViews trusted command access. `panel.json` retains its empty generic Tauri
permission list. Search operations pass through the actual actor command queue
and disposition ledger before the host hides on success. Failed admission stays
visible. The window, profile, space, session, and request all scope an action;
the action must also belong to the actor's offered results.

Browser tool entries use the `tools.presentation` preference (`follow_layout`
or `floating`). The current vertical layout uses its sidebar unless floating
is selected. Launcher destinations always use the panel. No horizontal-tab
implementation is included.

## Loading and shared views

`frame/src/main.ts` chooses the surface before loading either app graph. Main
mount and presentation flushing remain synchronous after its import. The panel
root includes search, metadata, theme/material, and shared visual foundations;
it does not import Settings or browser domains at startup.

`features/tools/manifest.ts` contains lightweight metadata and explicit dynamic
imports. `ToolSlot.svelte` mounts only the current feature and ignores obsolete
import completions. Both the sidebar and panel use that component, composed at
the app layer. Features receive state and callbacks, not native-window access.

Hiding the panel removes its search/tool subtree. Search listeners and its
single debounce/deadline are disposed; tools currently own no polling or service
workers. Drafts live outside mounted views, keyed by host/profile/tool, capped
at 24 entries per WebView with bounded strings. Sidebar and floating drafts are
independent. Module caching is normal: unmounting does not promise a return to
cold-process memory usage. Future data services must explicitly dispose their
subscriptions and consume committed Rust domain state.

Search waits for its listener before the first request. It coalesces typing,
suppresses requests during IME composition, rejects stale contextual results,
and retains selection by canonical action identity during result updates.
Native results are bounded and deduplicated; home shows six tools and at most
four recent browser items. Tool failures expose retry/back controls.

## Geometry and materials

Both routes keep one geometry: default 720 × 520, nominal minimum 560 × 360,
maximum 960 × 760, further bounded by usable monitor area and margins. Small
screens take priority over nominal minimums. Versioned geometry uses the
existing Rust store (`panel.geometry.v1`), bounded decoding, debounced writes,
and final hide/shutdown updates. Monitor-relative logical coordinates are
revalidated at restoration. Wayland restores size and leaves position to the
compositor; `position_restorable` reports this distinction.

Native material, clipping, and CSS consume the same 20 px radius. The window
owns the exterior shadow. No CSS blur or geometry animation is introduced.
Route content uses short opacity/translation transitions and respects reduced
motion; material handling retains reduced-transparency support.

## Verification record — 2026-09-11

- Frame typecheck, lint, formatting, 97 tests, and production build pass.
- App tests: 325 pass, including queued search-action admission.
- Desktop suite: 101 pass and 2 ignored before two additional owner/focus tests;
  the additional overlay tests are checked separately.
- Native Clippy across desktop targets passes with warnings denied.
- `frame/dist/bootstrap-report.json` traverses emitted static imports, including
  shared chunks. The build rejects eager browser domains, Settings, sidebar,
  or tool-view code in the panel graph. Current panel graph is approximately
  127 KB JS and 17 KB CSS (uncompressed); the main graph is approximately
  283 KB JS and 84 KB CSS. These are emitted-asset counts, not runtime memory.
- The original pre-split asset snapshot counted only its entry JS and is not
  comparable to this full static-graph count; no JS reduction percentage is
  claimed.

### Native acceptance still required

Computer Use could not inspect Zephium: macOS ScreenCaptureKit failed to start
capture (error -3811). Therefore the following are **not verified** by this
implementation pass:

- Actual keyboard/pointer and IME operation, focus restoration, import-failure
  recovery, light/dark, reduced transparency/motion, and accessible focus.
- Native radius/clipping, drag/resize, owned menu/dialog transitions, app
  activation, monitor changes, and geometry restoration after a restart.
- Windows and Linux behavior, including Wayland compositor and portal dialogs.
- Runtime WebView count, idle CPU, RSS, and warm-cache open/close-cycle trends.

For resource qualification, record the process tree, WebView count, CPU and RSS
at hidden cold start, visible search, each tool's first open, then hidden after
at least 20 repeated cycles with caches warm. Verify no continuing feature
activity while hidden; compare stable settled measurements rather than assuming
cached code will be unloaded. The structural single-window implementation and
passing source tests do not replace that live evidence. This is a review build,
not a declaration of production readiness.

### Stylesheet-loading correction

A subsequent native screenshot exposed unstyled launcher content despite the
static module-graph check passing. Vite combined a conditional dynamic import
into one preload wrapper containing only the main surface's CSS dependencies.
The router now has separate awaited imports and mount branches. The build guard
also checks final stylesheet preload references in `writeBundle`, after Vite
resolves them. Rebuilding with the original conditional import was verified to
fail this new guard; the corrected import branches pass. Native appearance
following this fix still requires review of the restarted build.
