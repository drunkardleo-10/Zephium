# Zephium UI renewal: audit and proposed delivery plan

Status: discussion draft, 2026-09-10. Pending layout answers and visual references.
This records the current UI brief and source inspection; it does not replace
accepted product or security contracts. No implementation is included.

## Brief

Make the browser interface a defining strength of Zephium: distinctive,
premium, modern, calm, and native in interaction as well as appearance.
Browse must be excellent with AI disabled. Work retains its separate product
identity; its execution, nodes, tasks, and internal workflows are outside this
implementation phase.

Preserve the successful input and segmented-control language, useful tab
treatment, Geist, and reusable design-system foundations. Reassess composition,
control consistency, and surface hierarchy. Remove Interface Studio entirely
during implementation. Settings becomes a real product surface for exercising
the system. The current app icon is outside scope.

The immediate deliverable is a coherent UI architecture and design plan. The
subsequent implementation should cover browser presentation and interaction,
with existing backend behavior connected where available. Missing browser
services need explicit integration contracts, not invented frontend authority.

## Findings from the current tree

| Area | Evidence | Consequence |
| --- | --- | --- |
| Product composition | `app/Shell.svelte` renders Sidebar and New Tab; selecting Work changes a sidebar body through local shell state | The current Work switch does not establish the full-window environment described in the product direction |
| Settings | `features/sidebar/panel/UtilityPanel.svelte` provides an empty Settings panel; `gallery/SettingsPage.svelte` is a local appearance fixture | There is no complete product Settings UI to preserve |
| Utility layout | `Sidebar.svelte` and `domain/shell/panel.svelte.ts` show a compact rail beside a panel; `docs/design/interface.md` says panels replace the tab body without adding a column | Choose and document one intended interaction; do not accidentally inherit either |
| Footer | `SidebarFooter.svelte` mixes the current profile, disabled account/profile actions, tools, appearance commands, and Settings | Separate profile identity from online account identity; move appearance into Settings |
| Navigation | `SidebarHeader.svelte` wraps navigation in `Cluster`; `.ui-cluster` adds a resting capsule fill | Remove that capsule from navigation; verify optical alignment against actual native window controls |
| New Tab | `NewTab.svelte` fixes the wordmark at 84 by 28 px, adds favorites and static tasks/time controls; the interface doc specifies a clock-centered page | Establish one composition based on the current brief, including a larger wordmark; remove unsolicited placeholder content |
| Primitives | `Dropdown.svelte` uses Bits UI Select; `Menu.svelte` uses Bits UI DropdownMenu; `Toggle.svelte` combines switch and checkbox with custom drag behavior | Bits UI adoption exists but is inconsistent; split distinct control contracts and retain Zephium styling |
| Menus | The shared menu portals inside the owning document, while native menu commands also exist | Audit containment at compact widths and window edges; DOM portals cannot cross native WebViews |
| Localization | Paraglide and English messages exist; several feature strings remain inline; compilation uses the base locale | Preserve the infrastructure and complete coverage, locale selection design, expansion, and RTL preparation |
| Documentation | The interface, system, and frontend docs contain conflicting composition rules and stale paths/visual values | Reconcile accepted decisions as part of each relevant change |

These are source-level findings, not a fresh visual or interaction sign-off of
the native app. Existing native material code is present, but its appearance
must be judged in the running browser, including light/dark and accessibility
fallbacks. The workspace already contains extensive uncommitted UI/native/icon
work; implementation must use focused edits and preserve unrelated changes.

## Visual direction to establish

- Use native window material as the foundation, with legible content surfaces
  and quiet resting controls. Preserve the no-CSS-blur direction.
- Make hierarchy deliberate: browser chrome, page canvas, settings groups,
  selected rows, and transient menus need distinct roles and contrast.
- Give the browser compact, precise controls and give content pages breathing
  room. One spacing system does not mean identical density everywhere.
- Tune typography, icon optical size, hit area, baseline, radius, and spacing
  together. Fix header geometry using native window measurements, not another
  isolated CSS offset.
- Define light and dark together. Material must remain readable over light,
  dark, and busy desktop backgrounds and in inactive windows.
- Use motion to explain selection, opening, and continuity. Avoid decorative
  idle motion, repeated entrance effects, and animation of native page geometry.
- Compose the supplied wordmark as an asset with intentional proportions;
  preserve its aspect ratio and tune its optical relationship to search.

Before extending the design everywhere, review three product compositions:
Browse with real tabs and a page, empty New Tab, and a full Settings page with
an open selection menu and visible keyboard focus. Screenshots establish
appearance; keyboard and pointer use establish interaction quality.

## Browser surface map

This is the target presentation inventory, not a claim of implemented services.
The exact Settings host and default utility placement remain open questions.

| Surface | Intended responsibility | States/interactions to specify |
| --- | --- | --- |
| Browser shell | Window controls, navigation, address/site identity, mode entry, space, tabs, extensions, profile | Default/compact/narrow window, inactive window, fullscreen, loading/stop, empty and crowded tab lists |
| Tabs and organization | Essentials, pinned/open tabs, folders, spaces, splits, archived/sleeping tabs | Selection, close, drag/drop, rename, context menu, audio, crash/retry, long titles, keyboard navigation |
| New Tab | Calm starting point and search using the retained field language | Empty input, focus, typing/submission, long/localized copy, appearance; optional content only by explicit design |
| Launcher and page tools | Search/commands, find in page, zoom and page actions | Empty/loading/results/no-results, selection, scope, shortcut hints, dismissal and focus return |
| History | Recent browsing and full history destination | Search, grouped dates, empty/error, selection, reopen, deletion confirmation; backend hookup separate |
| Downloads | Transient progress and a full download destination | Active, paused, completed, interrupted, retry, open/reveal, missing file; backend hookup separate |
| Profiles and spaces | Browser identity and organization | Active profile, switch/create/edit/remove, private availability, empty spaces; keep online sign-in distinct |
| Settings | Searchable browser preferences and management entry points | Navigation, control states, validation, pending/failed save, restart required, unavailable capability |
| Privacy and extensions | Site identity, permissions, protection, installed extensions | Request, granted/denied, degraded capability, update/enablement state; preserve exact existing authority |
| Account | Optional hosted identity, model-service access, usage and billing destinations | Signed out/in, unavailable/offline, expired session; local browsing remains independent |
| First use and recovery | Initial setup, import entry, default-browser choice, interrupted session and updates | Skip/cancel, progress, partial failure, restore, retry; do not invent completed operations |
| Work boundary | Mode placement, transition, return to Browse, disabled-AI behavior | Reserve the destination and future presentation contract; defer internal Work implementation |

Small tools such as print, save page, page zoom, reader/translation entry, and
developer tools need a discoverable command/menu location. Inclusion in the map
does not turn an unsupported engine capability into a promised feature.

## Proposed Settings information architecture

Use a small number of understandable top-level groups, with searchable
subsections and stable deep links. Avoid putting every setting in the sidebar.

| Group | Candidate subsections |
| --- | --- |
| General | Startup/session behavior, default browser, import/export |
| Appearance | Theme, material/accessibility behavior, density, sidebar, tabs, New Tab |
| Browsing | Navigation, search providers, downloads, languages/translation, media |
| Profiles and spaces | Profile management, spaces and their customization |
| Privacy and security | Site permissions, browsing data, blocker, credential management |
| Extensions | Installed extensions and their supported controls |
| AI and connections | AI enablement, hosted/BYOK/local models, connected tools and access boundaries |
| Keyboard and accessibility | Shortcuts, focus behavior, motion, text/contrast preferences |
| Performance | Sleeping tabs, resource policies, supported diagnostics |
| Account | Online identity, hosted usage, service management |
| About and advanced | Version/update state, advanced supported options, reset/recovery |

History and Downloads lists are destinations; their preferences live in
Settings. Model controls and account pages are presentation scope only where
their services are not yet connected. Pseudo-localization and stress controls
belong in development verification, not end-user Settings.

Each setting needs an explicit ID, localized label/help/search terms, scope
(application/profile/space/site), value type, default, availability, validation,
and save behavior. Scope must follow the authoritative domain rather than the
page on which the setting happens to appear. A small typed catalog can drive
navigation and search; use explicit Svelte page composition for actual layouts,
not a universal JSON form renderer.

## Frontend architecture

Retain the existing feature-based layout and enforce its dependency direction:

```text
app/                 surface routing and composition
features/            browser shell, settings, history, downloads, profiles, etc.
domain/              shared projections of authoritative Rust state
shared/ui/           Zephium-owned primitives and genuinely reusable patterns
shared/ipc/          generated commands/events and scoped transport helpers
shared/i18n/         generated messages and locale plumbing
styles/              semantic tokens, native material policy, shared control styles
```

Feature modules do not import each other. Components receive values and emit
intent; feature controllers bind them to the existing domain projections.
Transient focus, open menus, selection, and unsaved field drafts remain local.
Durable preferences, profile identity, permissions, and operation settlement
remain Rust-owned. Avoid a global mutable UI store containing every feature.

Give navigation a small discriminated surface model with one active destination
and explicit back/focus restoration. Keep browser mode, sidebar shape, utility
destination, and internal-page location distinct. Where native layout or page
visibility changes, Rust still admits and settles the presentation.

Native menu versus Bits UI is a surface decision. Use Zephium wrappers around
Bits UI for custom selection, menus, switches, checkboxes, and other behavior
where useful inside an authorized DOM surface. Native HTML remains appropriate
for inputs and buttons. Native menus remain necessary where the surface crosses
page pixels. Specify a single naming policy for Select versus action Menu;
avoid two vaguely interchangeable dropdown components.

Split Switch and Checkbox. Include keyboard operation, focus, disabled/read-only
states, mixed checkbox state, errors and descriptions. Assess switch dragging
as a deliberate interaction with pointer cancellation and RTL behavior, rather
than carrying the current label-wide drag handler forward automatically.

Use existing native material handling. Keep chrome/page WebViews non-overlapping,
preserve startup and tab-presentation sentinels, and regenerate IPC types from
Rust whenever the native contract changes. A trusted internal Settings page
requires explicit native admission and scoped capabilities; never give an
ordinary content tab the browser bridge to make routing convenient.

Lazy-load large destinations. Extend the current localization system with
complete string coverage, plural/number/date formatting, logical CSS, RTL and
long-label verification. Do not translate the catalog in this phase.

## UI-only work and future service integration

Interactive design should include navigation, menus, focus, search over settings
metadata, and editable presentation states. Use real backend adapters when the
feature already exists. Missing services can have bounded development fixtures
attached to the same feature view without a separate gallery application.
Fixtures must be excluded from shipped behavior and cannot write product data.

Record loading, empty, populated, unavailable, pending, failed, and settled
states alongside each feature's typed view model and emitted intents. Do not
show fabricated download/history/account data or a saved/connected/success state
in production. A missing backend action is not an ordinary empty state. Keep
unfinished destinations out of normal product navigation until their intended
availability treatment is designed. This phase can complete presentation while
remaining explicit that functional browser completion is separate.

## Delivery sequence and evidence

1. **Resolve composition.** Obtain visual references and answers below. Record
   the accepted shell, utility, identity, New Tab, Settings, and Work boundaries;
   reconcile the three design/frontend docs.
2. **Remove Studio.** Remove `frame/src/gallery`, `frame/gallery.html`, gallery
   Vite middleware/mode, root gallery scripts, native gallery runner/binary and
   Cargo feature, associated ignores, and documentation references. Keep shared
   primitives, localization, production material code, and independent checks.
3. **Establish shell and Settings foundation.** Introduce explicit navigation
   and minimal host integration where needed. Correct header geometry and
   navigation fill, simplify the footer, and build the agreed New Tab composition.
   Compose Settings with retained fields/segmented controls and refined Switch,
   Checkbox, Select, and Menu. Review these together in the native browser.
4. **Complete Settings presentation.** Build the accepted pages, navigation,
   search/deep links, form states and availability contracts. Keep styling and
   behavior shared without forcing all pages into the same row template.
5. **Complete remaining Browse presentation.** Apply the established patterns
   to the surface inventory, including first-use, empty, interrupted, and crowded
   states. Produce a per-feature integration handoff for missing services.
6. **Verify product quality.** Run the frame gate and relevant native checks;
   validate actual pointer/keyboard behavior, focus restoration, screen-reader
   semantics, narrow windows, light/dark, reduced motion/transparency, forced
   colors, RTL and text expansion. Compare startup, idle activity, memory and
   view count to the baseline. Platform evidence must name the tested OS.

The first visual review is intentionally early, before duplicating the design
across every destination. Completion means the agreed presentation inventory
and interaction states are covered, with native-browser evidence and a clear
list of service integration gaps. A polished screenshot or green build alone
does not establish that result.

## Pending product choices

1. Work: shell destination/transition only, or broader visual screen design?
   Current working scope is shell only, with internals deferred.
2. Settings: an in-window destination, a separate window, or an overlay?
   Proposed default is an in-window destination; its privileged host must be
   designed explicitly.
3. Platform sequence: macOS first with cross-platform contracts, or simultaneous
   visual development? The product release direction prioritizes macOS/Windows.
4. Utilities: compact rail beside a panel, tab-body replacement, or full page by
   default? This resolves the current code/documentation disagreement.
5. New Tab: larger centered wordmark and search only, optional widgets, or a new
   composition informed by the reference screenshots?
6. Footer identity: active browser profile with account access inside, or online
   account with a separate profile switcher? Proposed default is profile-first.

Needed visual references: the current header/alignment screenshot, the admired
browser/control screenshots, and identification of which visual qualities to
borrow. The supplied app icon should not influence this design review.
