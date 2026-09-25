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

## Search presentation refinement — 2026-09-16

New Tab keeps the original `SearchField` page capsule. Its shared search view
opens an anchored dropdown only while an entered query is active; it has no
launcher drag header, home tool grid, or footer. Escape dismisses the dropdown
without discarding the query. Focus leaving the control cancels the scoped native
request and pauses its UI deadline. Dropdown height is bounded by the available
viewport space and does not move the New Tab layout.

The launcher and New Tab share compact 34 px result rows, grouped by source.
Searches use a magnifier, history a clock, and notes a note icon; source names
appear once as group headings. Keyboard selection follows action identity through
incremental updates. Retained rows cannot execute while a replacement query is
pending. Empty intermediate responses retain the previous visual results; an empty
state is admitted only after native history and supplementary providers finish.
The native `SearchResults.pending` field carries this state; it is not guessed
from an arbitrary UI delay. Obsolete provider completion cannot settle a newer
query. The address bar remains a plain URL/search submission field.

Browser component tests now run the same Tailwind compiler as the production
build, so semantic token definitions and utility classes are present during visual
checks. Light/dark New Tab screenshots are emitted beneath `target/`. These tests
verify WebKit component behavior, not native window focus/material or cross-platform
qualification. Native app access was not granted by Computer Use during this work.

Search and New Tab remain lazy. The search lazy graph includes its shared parent
chunks; its budget measures that complete graph rather than an incremental download.
Existing browser/panel startup limits remain unchanged. No CSS blur, result-list
geometry animation, or new polling has been introduced.

## Search production pass — 2026-09-16

The shared view moved to `frame/src/features/search/`: `SearchSurface` behaviour
lives in `lib/search-surface.svelte.ts`, presentation in `ResultList` and
`ResultRow`, and `LauncherPanel` and `NewTabSearch` are thin hosts that own only
their own chrome. `features/launcher/` and its global `.panel-*` result
stylesheet are gone; result styling is component-owned and token-based.

### Ordering

`zephium_core::search::ResultSection` gives every result a fixed presentation
order (Search, Tabs, History, Notes, Commands) with a per-section capacity.
`publish_search` stable-sorts by section only, because each result is already
pushed in its section's own preference order. A late note or online suggestion
therefore extends its own run instead of re-ranking rows the user is reading.
The earlier flat score re-sorted and re-truncated the whole merged list on every
provider arrival, and truncated again before the merge, so the settled order
depended on provider timing.

What the user typed is always the first row and is the action Enter runs. A
prefix-matching tab title previously scored above the search row, so typing two
characters could switch tabs when a search was intended.

### Provider lifecycle

`desktop/src/search_providers.rs` keys in-flight work by search session, so a
New Tab field, the floating launcher and a second window no longer abort each
other. A `Completion` guard dispatches `SearchSupplementaryFinished` on every
exit path including `abort()`. Previously an early return left
`supplementary_pending` set permanently, and because the New Tab list only opens
once results exist or pending clears, a query with no local match never opened
its list at all. Input that cannot produce supplementary results settles without
spawning a task.

### Storage

`search_history` bounds its scan by recency and then groups, instead of capping
FTS candidates before the group-by. `history` holds one row per visit, so the
previous 256-row candidate cap was consumed by whichever address was visited
most, hiding every other match. Ranking is now frecency (visit count weighted by
recency bucket) rather than `last DESC`. No `history_fts` prefix-index migration
was added: rebuilding that index on an existing profile is a blocking startup
cost that the bounded scan makes unnecessary.

### Measured

Native search requests for typing "rust language" (13 characters), driving the
real controller under fake timers:

| Keystroke interval | Before | After |
| --- | --- | --- |
| 30–50 ms | 13 | 2 |
| 60 ms and slower | 13 | 13 |

The previous scheduling was `setTimeout(…, 0)`, which is one request per
character at any speed. The 60 ms coalescing window therefore bounds the request
*rate* at roughly 16/s rather than removing per-keystroke requests for ordinary
typing; deliberate typing still queries per keystroke, as shipping browsers do.
Expensive supplementary work is bounded separately: the note index and the
network are reached only after a 140 ms quiet window, so a burst aborts the task
before it queries either.

Emitted graphs: `LauncherPanel` 118,213 B JS / 5,848 B CSS, `NewTabSearch`
117,424 B JS / 5,727 B CSS. No existing budget was raised. The New Tab clear
control is placed by its own host rather than threaded through the shared
`SearchField`, because widening that primitive pushed `PrivacyPage` past its
budget for one caller's benefit.

### Field behaviour pass

New Tab offers places to go, not actions: browser commands are excluded when the
bound session is a New Tab one and stay in the launcher, which is the surface
built for them. Notes remain, as a destination rather than a command.

Enter follows what the field is showing. Once an inline completion has been
applied the field reads `notion.so`, so that is the row selected and the row
Enter opens; searching the fragment that was typed is the wrong answer.
`completionTarget` resolves the offered host back to the row that supplied it,
accepting either spelling native uses for a host — a tab carries its bare host,
a visit its full address.

Emphasis is measured against `SearchSnapshot.answered`, the query the visible
rows actually answer, rather than the live text. Measuring against the live text
made a row's bold match drop out for the moment between a keystroke and its
answer, which read as the list lagging behind the typing.

The provider windows were split. One shared window meant every suggestion also
waited out the note-index window, so the fixed delay before leaving the machine
was 180 ms; it is now 60 ms, while the note index keeps a 130 ms window and the
resource-bounding property that comes with it. The remaining wait is the round
trip itself.

The list animates its own height to its content, measured on the border box so
the padding is included — a content-box measurement leaves a permanent
scrollbar on a list that fits. There is deliberately no per-row entry animation:
the typed row's action carries the query, so its identity changes on every
keystroke and a fade would re-run as fast as the user types, reading as exactly
the lag it was meant to soften.

### History crowding

Recorded searches and visited pages both live in the History section. Ranked
together under one shared cap of three, a browser that has been used for a
while filled every slot with its own past queries and stopped offering pages
at all — which reads as history search being broken, on both surfaces, getting
worse the more the browser is used.

Capacity is now counted per kind (`kind_capacity`) rather than per section: a
section can mix sources worth very different amounts, and the cheaper one must
not be able to spend the whole budget. A past query is not a destination and
retyping it is cheap, so it gets one slot; visited pages get four. The reader
also asks for ten candidates rather than six, so dedup against already-open
tabs cannot leave the section short.

`history_reaches_the_field_through_the_real_store_and_read_queue` covers the
seam this hid behind: a real profile database, the real read queue and the real
worker loop, driven by the scoped command the shipping surfaces send. Each piece
had coverage; the path between them had none.

### Completion and deletion

Deletion is judged against what the field is showing, not against the typed
text. With a completion applied the field reads `youtube.com` while the typed
text is `you`; deleting the completion leaves the typed text the same length,
so a length comparison against it saw no change, allowed the completion to be
re-applied, and backspace did nothing however many times it was pressed.

The field stays bound to the typed text. Binding it to the displayed text was
tried and reverted: Svelte writes `input.value` whenever its tracked value
differs from the prop, and that write collapses the selection that makes the
next keystroke replace the offer.

### Not verified

- No native app run. Light and dark checks are WebKit component screenshots
  under `target/search-newtab-*.png` and `target/search-launcher-*.png`, not
  native window, material, focus or IME behaviour.
- No cross-platform qualification, process-memory benchmark, or end-to-end
  latency measurement. The table above counts requests, not latency.
- Clippy still reports the pre-existing `ResourceCall` `large_enum_variant` and
  `api.rs` `type_complexity` warnings. The repository release gate is not clean.
