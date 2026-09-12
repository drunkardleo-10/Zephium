# Frame architecture foundation (frontend production-grade baseline)

Status: FINAL, 2026-09-11. Reviewed with the product owner and cross-reviewed by the Work
runtime track; all decisions below are settled. Executed by a fresh implementing agent phase by
phase. No visual changes are part of this plan.

Read first: this file, then `docs/frontend.md`, `docs/product-system.md` (sections 7, 8, 11),
`docs/architecture.md` section 11, `docs/work-runtime-continuation.md` (if present in this
checkout; it is the Work runtime handoff). The implementing agent must not touch the Work
runtime crates or design Work contracts; see section 8 and step 4b.

Joint contract status: it does not exist yet. The Work runtime track is on another machine
and the product owner coordinates when it is written. Only step 4b waits for it.

Implementation evidence and remaining gates: [frontend-architecture-progress.md](frontend-architecture-progress.md).

Further module-internal and Work presentation scope is proposed in
[frontend-module-work-foundation.md](frontend-module-work-foundation.md). Its revised
test placement and fixture track are pending review, not implemented by this plan.

## Context

Zephium's Rust side (core / app / engine / store / desktop) has a deliberate architecture:
functional core, one actor, typed projections, colocated tests, fail-closed gates. The Svelte
frame (`frame/`) grew feature by feature while the backend was the focus. It is not bad: the
four-layer split (app / features / domain / shared) already exists, dependency direction is
clean (verified: zero cross-feature imports, domain and shared never import features), IPC is
generated, projection admission is framework-free and tested, and the launcher graph is
build-guarded. But the foundation has nine structural problems that will compound as Settings,
Work (XY Flow canvas, charts, dashboard), themes, and outside contributors arrive:

1. No single surface/navigation model. Shell state is five ad-hoc singletons in
   `domain/shell/` (sidebar-mode, tools, browser-page, tab-drag, motion) importing each other.
2. `UiCommand` is a bare `string` on both sides (`crates/zephium-ipc/src/lib.rs:996`
   `UiCommand(String)`, `bindings.ts:890`), parsed with `startsWith`/`slice` in 18 places
   across theme, preferences, tools, browser-page, Shell, PanelApp.
3. Preferences are read with one IPC call per key at startup and confirmed by a 15x100ms
   readback poll after every write (`domain/preferences/preferences.svelte.ts`). Rust already
   owns the key/value catalog (`crates/zephium-core/src/preferences.rs`) but never projects it.
4. CSS is split-brain: Tailwind for layout utilities in 20 files, real styling in 3.1k lines
   of global CSS (`styles/browser.css` 1611, `components.css` 968, `panel.css` 321), one scoped
   `<style>` in the entire tree. No ownership, no dead-CSS detection.
5. `domain/` has drifted from its own definition (doc: "mirrors of authoritative Rust
   projections"). It holds UI-only state, while a real Rust projection
   (`features/split/layout.svelte.ts`) lives in a feature.
6. Folders by screen region: `features/sidebar/` holds extensions (1261-line
   `ExtensionManager.svelte`), shield, essentials, spaces, address.
7. Tests are a flat pile of 24 files in `frame/tests/`, each hand-rolling `vi.mock` of the
   bindings; no `vitest.config`; no component tests.
8. Boundaries are convention only: no lint enforcement, no path aliases (every import is
   `../../shared/...`), no dead-code detection.
9. Rust `include_str!`s 20 frontend paths in a 6k-line `desktop/src/lib.rs`; a shadow
   contract that makes every file move a Rust change.

The goal is a foundation where a contributor or agent can tell, without asking, where a file
goes, what it may import, where its styles and tests live, what Rust owns, and how a new
surface (Work, an internal page) is added without touching the rest.

## Governing principle: ownership

**Rust owns** anything that persists, changes native geometry or visibility, is a security
decision, or is shared across windows/surfaces: preferences, themes as data, items/tabs, which
browser page is open, sidebar width, operation settlement, Work state.

**The frame owns** state that lives exactly as long as one WebView document: focus, open
menus, hover, drafts, transition phases, scroll positions, which sidebar slot body is shown
(with its width consequence sent as an intent). It never claims durable success; it renders
projections and sends typed intents.

Everything below follows from that split.

---

## 1. Layering and folder structure

Five layers, strict downward imports, enforced by ESLint (section 6).

```
frame/
  browser.html  panel.html                      one HTML entry per native surface (section 7)
  src/
    entries/       browser.ts, panel.ts          mount only; no logic
    app/           composition roots per surface
      browser/     BrowserApp.svelte, Shell.svelte, shortcuts.ts
      panel/       PanelApp.svelte
    features/      vertical slices by PRODUCT CONCEPT, never by screen region
      sidebar/     the frame only: header, footer, resize handle, mode switch, body slot
      tabs/        tab list, rows, folders, split-group rows, tab menu
      essentials/  rail, tiles, drop target
      address/     address field
      spaces/      space header, switching
      extensions/  manager, action icons, permission prompt
      blocker/     shield
      permissions/ page permission prompt
      launcher/    search UI + controller; ask/ route reserved (8.3)
      newtab/
      settings/    pages/, components/, catalog (presentation metadata only)
      tools/       manifest, ToolSlot/ToolFrame, views/
      library/     history + downloads destinations
      split/       dividers
      tasks/ notes/ focus/ activity/  entity features, host-agnostic presentations (8.1, 8.2)
      work/        Work-only sub-features as peers: home, canvas, plan, runs (section 8)
    session/       frame-owned transient state shared by 2+ features (NOT Rust mirrors)
      surface.svelte.ts   the one navigation model (section 4)
      drag.svelte.ts      tab drag transient state (from domain/shell/tab-drag)
      motion.svelte.ts    transition phases (from domain/shell/motion)
      tool-drafts.svelte.ts  (from features/tools/session.svelte.ts)
    domain/        ONLY mirrors of Rust projections + typed intents, one folder per Rust slice
      tabs/ preferences/ appearance/ blocker/ extensions/ permissions/ credentials/
      runtime/ operations/ layout/ panel/ surface/ commands/ ask/ (reserved)
    shared/        cross-cutting, no domain opinion
      ipc/         generated bindings.ts, native-events.ts
      blocks/      block-spec renderer, the content vocabulary (8.4); reserved
      i18n/        generated paraglide output (unchanged)
      ui/          visual primitives, one folder per primitive (section 5); ui/data for
                   Table/Chart/Stat/KeyValue/Timeline
      lib/         lifecycle.ts, clamp/debounce/ids, small pure helpers
      platform/    IS_MAC etc.
      testing/     mock bindings factory, event emitters, fixtures (section 6)
    styles/        tokens/, axes/, base.css, material.css (section 5)
```

Rules:

- **Import direction**: `app -> features -> session -> domain -> shared`. A layer imports only
  itself and layers below. Features never import other features. `shared` imports nothing above.
- **Public API per module**: every `features/<x>/` and `domain/<x>/` exposes `index.ts`; deep
  imports across module boundaries are forbidden (lint). Inside a module, relative imports.
- **Composition by snippets**: `Sidebar.svelte` renders header/footer/resize and exposes body
  and rail snippets; `app/browser/Shell.svelte` fills them from `features/tabs`,
  `features/essentials`, `features/extensions`, etc. This is the existing pattern (Shell already
  passes `settingsNavigation` and `toolPanel` snippets); extend it instead of nesting features.
- **Where does a store go?** Mirrors Rust -> `domain/`. Frame-owned and used by 2+ features ->
  `session/`. Used by one feature -> that feature. Pure and reusable -> `shared/lib`.
- **File naming inside a module**: `<name>-model.ts` (pure, framework-free), `<name>.svelte.ts`
  (runes store with `init()/dispose()`), `<Name>.svelte` (components), tests colocated
  (section 6). Components in a feature with more than ~6 files go under `components/`.

Moves (representative, the implementer enumerates the rest):

| From | To |
| --- | --- |
| `domain/shell/sidebar-mode.svelte.ts` | `features/sidebar/mode.svelte.ts` (mode is sidebar-local; width publish moves to surface) |
| `domain/shell/tools.svelte.ts`, `browser-page.svelte.ts` | folded into `session/surface.svelte.ts` + `domain/surface/` (mirror of `PrimarySurfaceState`) |
| `domain/shell/tab-drag.svelte.ts`, `motion.svelte.ts` | `session/` |
| `domain/ui-commands/` | dissolved by typed events (section 3) |
| `domain/preferences/preview.svelte.ts` | `features/settings/preview.svelte.ts` (settings-only draft state) |
| `domain/theme/theme.ts` | `domain/appearance/` (consumes preferences projection + material event) |
| `features/split/layout.svelte.ts` | `domain/layout/` (it is a Rust projection) |
| `features/sidebar/{tabs,essentials,address,space,extensions,shield}/` | `features/{tabs,essentials,address,spaces,extensions,blocker}/` |
| `features/settings/settings-state.svelte.ts` | stays (feature-local) but selection driven by surface model |
| `features/tools/session.svelte.ts` | `session/tool-drafts.svelte.ts` (sidebar and panel both use it) |

Rust side, same commit as each move: `desktop/src/frame_sources.rs` with one
`pub const` per `include_str!` of a frame file; every markup test references the constant.
After this, a move touches exactly one Rust line.

---

## 2. Store conventions (domain and session)

Every `.svelte.ts` store keeps the proven shape (idempotent `init()/dispose()`, generation
guard, listeners installed before the first `await`, getters not exported state) but stops
re-implementing it. Extract:

- `shared/lib/lifecycle.ts`: `createLifecycle()` returning `{ begin(): generation,
  isCurrent(generation), end() }` plus `listenAll(listeners[])` (the `resolveListeners`
  logic from `domain/tabs/tabs.svelte.ts`). Each store's init drops to its domain logic.
- `domain/operations/settle.ts` (domain layer, exported from `domain/operations` public API;
  it needs `waitForDisposition`, so it cannot live in `shared/`): `settle(admission, timeoutMs)`
  returning a typed outcome `applied | no_op | deferred | rejected | failed`. Today this block
  is copied in `browser-page`, `preferences.set`, `tab-drag.move`. `deferred` is admission,
  never verified completion (the Rust contract says so); callers that need completion wait for
  the projection carrying the change.
- Projection stores expose: read getters, `init/dispose`, and intents (thin `commands.*`
  wrappers). Nothing else. No DOM writes except the appearance store (section 3).
- Presentation-barrier stores (`domain/tabs`) keep `flushSync` and the framework-free model
  exactly as they are; the barrier rules in `docs/frontend.md` are unchanged.

---

## 3. IPC contract hardening (Rust + frame)

### 3.1 Scoped typed projections replace the `UiCommand(String)` channel

`Projection::UiCommand(String)` in `crates/zephium-ipc` is deleted, not replaced by one large
enum. Each concern becomes its own `Projection` variant with its own specta type, and the
desktop dispatcher emits one scoped DOM event per projection so each domain store listens only
to its own. Every projection that can be missed carries a `revision` and is re-sent as a full
snapshot on bootstrap (the `ItemsState` pattern); stores listen before they query.

| Projection (Rust) | DOM event | Consumer |
| --- | --- | --- |
| `AppearanceState { appearance, material }` | `zephium:appearance` | `domain/appearance` |
| `PreferencesState { revision, values }` | `zephium:preferences` | `domain/preferences` |
| `PrimarySurfaceState { revision, surface }` (section 4) | `zephium:surface` | `domain/surface` |
| `CommandsState { revision, commands }` (3.3) | `zephium:commands` | `domain/commands` |
| `ToolRequested { tool: ToolKind }` | `zephium:tool-requested` | `session/surface` |
| `SettingsSection { section }` | `zephium:settings-section` | `features/settings` via surface |

Implementer inventories every `Projection::UiCommand(` emission in `crates/zephium-app` and
`desktop/src` and maps each to one of these; any remaining free-form id becomes an explicit
projection, never a string. `native-events.ts` gains the typed entries; `events.uiCommand` is
deleted along with all 18 string parsers. Intents stay as individual typed commands.

### 3.2 Preferences become a projection

Rust already validates keys and values in `zephium-core::preferences`. Add:

- `PreferenceKey` enum + typed value type exported through specta (no more `string/string`).
- `PreferencesState { revision, values }` projected at bootstrap and after every applied
  change (same snapshot + revision admission pattern as `ItemsState`).
- `settingSet` stays as the intent with operation admission; the frame marks the key pending
  until either the projection carrying the new value arrives or the disposition fails. The
  readback poll is deleted.

The Rust catalog is the source of truth for id, scope (app / profile / space / site), type,
default, validation. `features/settings/catalog.ts` keeps only presentation metadata
(labels, help, search terms, page placement) and must not duplicate defaults or validation.

### 3.3 Commands mirror

Product-system section 11: launcher, palette, native menus and the keyboard settings page
dispatch the same stable command ids from the Rust commands registry. Today the frame has no
mirror; the launcher and `App.svelte` shortcuts hardcode ids. Add a `CommandsState` projection
mirrored in `domain/commands`, consumed by the launcher, the shortcut table, and the keyboard
settings page. Each descriptor is `{ id: CommandId, text: LocalizedText, binding, available }`.

Localization contract (decided, applies to both sides):
- One neutral catalog: the existing Paraglide source under `frame/messages/`. It is the single
  source for frame and native strings.
- Generated typed accessors on both sides: Paraglide for TypeScript (exists); a Rust build step
  (build.rs or xtask, checked for drift in CI like `bindings.ts`) generating typed accessors
  from the same files for native menus and dialogs.
- `CommandId` is a stable identity and is not a message key. Rust never sends finalized
  English over IPC.
- `LocalizedText = { key, args } | { literal }`: `key` for product commands, `literal` for
  trusted dynamic product data such as an extension's own action name.
- Locale is a Rust preference in `PreferencesState`; the frame switches Paraglide's runtime
  locale from the projection (today the build uses the base-locale strategy only), and native
  menus rebuild on change.

### 3.4 Appearance store owns `<html data-*>`

One store (`domain/appearance`) applies every axis attribute to `document.documentElement`:
`data-theme`, `data-tint`, `data-material`, `data-density`, `data-contrast`, `data-text`,
`data-reduce-motion`. Inputs: preferences projection, material event, system media queries.
Today Shell.svelte, preferences.svelte.ts, theme.ts, and PanelApp each write attributes; after
this, nothing else touches `documentElement`.

---

## 4. Surface model (navigation)

`session/surface.svelte.ts` is the one read model for "what is the browser window showing":

```ts
// Rust-owned, projected as PrimarySurfaceState { revision, surface } (mirrored in domain/surface)
type PrimarySurface =
  | { kind: "browse" }
  | { kind: "page"; page: BrowserPage; section?: string } // chrome full-window page (settings/history/downloads)
  | { kind: "work"; workId: WorkId; revision: string }   // chrome full-window Work (section 8)

// frame-owned, composed in session/surface.svelte.ts
type Surface = PrimarySurface | { kind: "tool"; tool: ToolKind } // chrome body swapped to a tool

type LayoutMode = "sidebar" | "top";                    // mirrored from Rust core `layout::Mode`
```

- Rust owns the primary surface because it changes native layout and, for Work, because Work
  is a durable aggregate identified by `WorkId`; the chrome-hosting mechanism (today the
  browser-page presentation in `browser_pages.rs`) is an implementation detail behind it.
  `LayoutMode` comes from the layout projection. `tool` is frame-owned.
- The Work variant's semantics (where the revision originates, current-Work selection,
  deleted or unavailable Work, restoration, open/leave/switch intents, stale-projection
  recovery) are joint-contract content (step 4) and are NOT designed here. Until the contract
  exists the frame implements `browse`, `page`, `tool` and reserves `work`.
- Layout-mode agnostic from day one: `features/sidebar/` is the chrome frame, vertical is its
  first shape. Nothing in `session/` or `domain/` says "sidebar"; the extent intent
  (`chromeSetExtent`, below) is called from exactly one function in the surface model. Top
  layout is designed for, not built.
- Intents: `openTool(kind)`, `closeTool()`, `openPage(page, section?)`, `openWork()`,
  `returnToBrowse()`. The queued "open tool after returning from a page" logic from
  `tools.svelte.ts` lives here once, as an explicit `pending` field, not scattered listeners.
- Geometry consequence is one function: `chromeExtent(surface, mode, shape)` -> one command.
  Sidebar mode (`features/sidebar/mode.svelte.ts`) reports its shape; surface publishes the
  extent. Today `setPanelExtent` and `publish()` are split across two modules.
- Layout contract (Rust, step 4a). Today `LayoutState` carries only dividers. It becomes
  `LayoutState { revision, mode: LayoutMode, chrome_extent, constraints: { min, max },
  applied_seq, dividers }`, re-sent as a snapshot on bootstrap. The extent intent becomes
  `chromeSetExtent(extent, seq)` (the current `sidebarSetWidth` generalized); Rust ALWAYS
  answers an extent intent with a layout projection carrying that `seq` as `applied_seq` and
  the applied (possibly clamped) extent, including no-op and rejected cases.
- Geometry rule (Rust owns applied geometry; gestures may preview). Three states:
  Preview: during a pointer gesture the frame renders immediately within the projected
  constraints. Applied: intents are coalesced to at most one per animation frame (the existing
  rAF pattern), each with an increasing `seq`; Rust applies native geometry asynchronously.
  Committed: on release the frame sends a final intent with `seq = S` and adopts the layout
  projection whose `applied_seq == S` as the settled value; projections with other sequences
  during the wait are ignored for settlement; it never keeps its own last preview. Pointer
  cancel, lost capture, and a clamped or rejected resize resolve the same way: the frame
  adopts the projection answering its last `seq`. Unit-tested in `session/surface`.
- Escape/back semantics and focus restoration are defined here (renewal plan asks for this).
- `app/browser/shortcuts.ts` keeps the chrome shortcut table (from App.svelte) and dispatches
  through `commands.runCommand` or surface intents; App.svelte stops owning logic.
- Every lazily loaded surface (`page`, `work`, tool views) mounts inside a Svelte 5
  `<svelte:boundary>` with a fallback snippet, and reports a bounded, content-free error to
  Rust through one command for the local log. Scope is exact: the boundary catches rendering
  and effect failures only, not event handlers, async work, or the renderer. It limits
  blast radius for that class; it is not crash isolation (see section 8).

---

## 5. Styling system

Decision: **component-scoped styles on a semantic token API**, Tailwind v4 retained for
`@theme` tokens and layout utilities only. This is what makes themes, modes, density, and
contrast composable, and what lets a contributor find a component's styles in the component.

### 5.1 Token tiers

```
styles/
  tokens/
    primitive.css   raw palette, spacing scale, type scale. NEVER referenced by components.
    semantic.css    the theme API: --color-canvas/surface/raised/text/muted/accent/border...,
                    --radius-*, --shadow-*, --motion-*, --font-*, --control-*  (from tokens.css)
    components.css  optional component tokens (--field-height ...) that map to semantic
  axes/
    theme-light.css  [data-theme="light"] semantic overrides (from tokens.css)
    tint.css         [data-tint=*] accent overrides
    density.css      [data-density=compact] control/spacing overrides
    contrast.css     [data-contrast="true"] + @media (forced-colors)
    motion.css       [data-reduce-motion="true"] + @media (prefers-reduced-motion)
  base.css          reset, fonts, focus ring, scrollbars, sr-only, html/body/#root
  material.css      [data-material=*] chrome background policy (from global.css)
  app.css           @imports in layer order (browser entry); panel.css imports a subset
```

Rules:
- Components use semantic tokens only. Raw colors, hex, `rgb()`, `white/10`-style utilities,
  and primitive tokens are forbidden in components (stylelint, section 6).
- Semantic token names are a stable, documented contract (`docs/design/tokens.md`): user
  themes (later, Vivaldi-grade) are Rust-owned validated color records projected and applied
  as `style.setProperty` on `<html>` for semantic tokens only. No CSS payloads ever
  (product-system forbids executable theme payloads). This is why the primitive/semantic split
  matters: a theme replaces semantic values, never markup or rules.
- Each axis is one file; adding an axis (e.g. `data-scale`) is one file + one attribute in the
  appearance store.

### 5.2 Component styling

- Every `.svelte` owns its styles in `<style>` (Svelte scoping). Variants and sizes are
  `data-variant` / `data-size` attributes styled in that block, not class-string builders.
- Tailwind utilities are allowed for layout only: flex/grid, gap, padding/margin from the
  spacing scale, sizing, `min-w-0`, `shrink-0`, `sr-only`. Not allowed: color, radius, shadow,
  typography, border utilities (those go through tokens in scoped CSS).
- `shared/ui/<Primitive>/` one folder per primitive: `Button.svelte`, `Button.svelte.test.ts`,
  optional `button.ts` for shared types. `.ui-*` global classes from `components.css` move into
  the owning primitive's scoped block. Primitives accept `class` for layout placement only and
  never import domain/session/IPC (existing rule, now lint-enforced).
- Bits UI stays narrow: `Select` (Bits Select), `Menu` (Bits DropdownMenu), `Switch` and
  `Checkbox` split (renewal plan). One naming policy: `Select` = value choice, `Menu` = actions;
  `Dropdown.svelte` is removed.
- On the shadcn question: adopt its ergonomics (copy-owned primitives, typed variants, one
  folder per primitive), not its styling model (Tailwind class strings + `tailwind-variants`).
  We already own the visual system; importing shadcn-svelte would import its look and a
  variant-builder dependency for no gain.

### 5.3 Migration

`browser.css` (1611 lines) is deleted by moving each block into its owning component;
`components.css` into `shared/ui`; `panel.css` into the launcher/tools components with the
panel entry importing `base + tokens + axes` only. `global.css` becomes `base.css` +
`material.css`. The Rust "tonal chrome" gate (`chrome_styles_stay_tonal...` in
`desktop/src/lib.rs`) currently `include_str!`s source CSS; it moves to `cargo xtask
check-frame-styles` scanning the **built** `frame/dist/*.css` (no `backdrop-filter`, no
`filter: blur`, shadow blur radius bound). Stronger (covers scoped styles) and path-independent.

---

## 6. Tooling, boundaries, and tests

### 6.1 Path aliases

`tsconfig.json` `paths` + Vite `resolve.alias`: `$app`, `$features`, `$session`, `$domain`,
`$shared`, `$styles`. All imports across modules use aliases; inside a module, relative.

### 6.2 Lint-enforced boundaries

- `eslint-plugin-boundaries`: element types `app | features | session | domain | shared`,
  allowed edges exactly as section 1. Features may import only their own folder among features.
- `no-restricted-imports`: deep imports into another module (`$features/*/**` except `index`),
  `@tauri-apps/api/event` anywhere (documented security rule, now enforced),
  `@tauri-apps/api/core` outside `shared/ipc`, `$shared/ipc/bindings` from `shared/ui`.
- `@typescript-eslint/no-explicit-any` back on (bindings.ts already excluded).
- Phased enforcement: every rule lands in step 1 at full strength with scoped overrides for
  code whose migration is a later step; each override names that step in a comment. The step
  that migrates the code deletes its override in the same commit. Step 8 verifies no override
  remains. No blanket or permanent exemptions; every commit stays green.
- `knip` for unused files/exports/dependencies, run in `check`.
- `stylelint` with `stylelint-config-standard` + `postcss-html` for `.svelte`:
  `declaration-property-value-disallowed-list` forbidding hex/rgb/hsl colors outside
  `styles/tokens/` and `styles/axes/`; `custom-property-pattern` restricting component
  references to semantic names.

### 6.3 Tests: colocated, two projects

Decision: **colocated tests, at most one test file per source file**, no `tests/` directory.
This is a placement rule, not a coverage rule: test behavior and contracts (admission, state
transitions, security seams, keyboard and focus), never write mechanical mirror tests for
files with no behavior. Colocation is what the Rust side does, it keeps deletion and moves
honest, and it fixes the flat-pile problem: a test lives beside the thing it tests.

- `foo-model.ts` -> `foo-model.test.ts` (node environment, pure).
- `foo.svelte.ts` -> `foo.svelte.test.ts` (runes compiled by the Svelte plugin; node env).
- `Foo.svelte` -> `Foo.svelte.test.ts` (browser project).
- Feature-level scenario tests (multiple modules together) are the single
  `features/<x>/<x>.test.ts`; no `__tests__` folders.
- `vitest.config.ts` with two projects: `unit` (`environment: node`, `**/*.test.ts` excluding
  component tests) and `component` (Vitest browser mode with Playwright; provider `webkit` on
  macOS, `chromium` on Windows/Linux CI, matching the real engines) using `vitest-browser-svelte`.
  Component tests cover `shared/ui` primitives (keyboard, focus, disabled, forced-colors
  attributes) and the security-sensitive markup seams (sentinel attributes, label sentinel).
  Browser-mode tests approximate the engines; they do not replace the bundled WKWebView and
  WebView2 qualification (native focus, lifecycle, IPC, CSP, process behavior), which stays in
  the native CI runs.
- `shared/testing/`: `mockBindings(overrides)` factory and `emitNativeEvent(name, payload)`
  so tests stop hand-rolling `vi.mock("../src/shared/ipc/bindings", ...)` per file; typed
  fixtures for `ItemsState`, `PreferencesState`.
- `pnpm run check` = typecheck, lint, stylelint, format:check, knip, unit tests. Component
  tests run in CI on the matrix (`pnpm test:component`), not in the local pre-commit gate.

---

## 7. Surfaces: one HTML entry per native surface

Today `index.html` + `main.ts` branch on the window label at runtime, and a Vite preload bug
already bit once (launcher CSS). Change to Vite multi-entry: `browser.html` -> `entries/browser.ts`,
`panel.html` -> `entries/panel.ts`; Rust window creation points each window at its own URL
(`desktop/src/overlay.rs` for the panel, `tauri.conf.json` for main). Gains: the panel graph
cannot include browser code by construction, `bootstrap-report.ts` becomes a per-entry budget
table, and a future isolated surface is one HTML + one entry. CSP is unchanged
(`script-src 'self'`). Capability manifests stay per window label. Rust
`include_str!("../../frame/src/main.ts")` re-anchors to `entries/browser.ts` via `frame_sources.rs`.

Budgets are ratchets: `bootstrap-report.ts` holds, per entry and per named lazy chunk
(`work`, `settings`, each heavy library chunk), a maximum raw JS and CSS size and a
forbidden-module list. Initial values = current size + 10%; the build fails when exceeded;
budgets are only ever lowered, with one reviewed exception: a commit that raises a row must
touch only the budget file plus the change that needs it and state the reason in its message.
The static import-graph check per entry remains; multi-entry does not replace it. The panel row already exists structurally; this adds numbers and
the browse and Work rows.

Rule for hosting a UI surface:
- **Chrome-hosted (default).** A lazily imported chunk shown through the Rust-admitted
  browser-page presentation (`crates/zephium-app/src/shell/browser_pages.rs`): Settings,
  History, Downloads, Work. Zero cost until first use, unmounted on leave, mounted in an error
  boundary. Resolves the interface.md vs code conflict in favor of code.
- **Stage-hosted (escape hatch).** A zone-2 WebView in the content region, own entry, only
  when the content is not fully trusted or must be its own document: reader view of a page,
  print preview, extension popups. Never "because it is heavy".

---

## 8. Work environment (decision, no implementation)

Decision: **Work is chrome-hosted.** It is a lazily imported chunk shown when the Rust-owned
primary surface is `work` (section 4), unmounted when the user leaves, mounted inside an error
boundary. Work itself is a durable Rust aggregate (`WorkId`); hosting is presentation.

Why, over a separate WebView:
- Before first use, zero bytes loaded; product-system 8.1 "zero heavyweight overhead with Work
  off" holds exactly. On leave, unmount makes DOM and heap eligible for reclamation; retained
  module code (a few MB for canvas and chart libraries) cannot be unloaded from any document.
- Substantially lower composition, focus, accessibility and resource-management complexity
  than a second WebView in the stage.
- A separate WebView would give DOM, controller and lifecycle separation but not guaranteed
  process-crash isolation: WebView2 assigns renderer processes by site and may share them
  across instances; WebKit has its own process model. Neither sharing nor isolation is
  assumed for either option.
- When Work later shows a live page beside it, that is a layout mode (chrome rect + content
  rect) the tiling engine already supports, not a new surface.

ADR wording (`docs/adr/0001-work-hosting.md`): "Chrome-hosted Work is the current measured
default. Neither process sharing nor isolation is assumed. Reconsider if bundled measurements
reveal unacceptable memory, stability, security or restoration behavior." The ADR records
measurements from bundled macOS and Windows builds: memory before/during/after Work, crash
behavior, restoration latency, background resource use.

Accepted risks and mitigations: a renderer crash in chrome kills the UI (bounded and
virtualized projections per product-system 8.1; heavy compute in Rust or a worker; error
boundaries for render/effect failures only); Work code runs in the privileged document (typed
artifact blocks only, no `{@html}`, CSP script-src self, all unchanged).

Unmount discipline and retention (defined now, built with Work):
- On leave, Work explicitly releases listeners, timers, observers, workers and retained
  references; unmount is verified by measurement, not trusted.
- Durable semantic state (nodes, resources, plans, tasks, artifacts, approvals, relationships)
  is Rust-owned and never in the frame.
- Recoverable presentation state (camera, collapsed/expanded groups, last selection, viewport,
  settled layout) is `WorkView` in Rust (product-system section 7): bounded, debounced
  snapshots, outside the semantic audit log, scoped by Work plus window (later device).
- Gesture-only state (pointer position, active drag, hover, animation progress) is
  `session/work` and disposable.
- Draft text is user data, not view state: autosaved to Rust separately, never held only in a
  frame session cap.

Where Work lives. Work is a host, not a UI world. The entities inside it (tasks, notes,
resources, history, artifacts) are the same domain entities the browser has, each with one Rust
projection and several presentations. The tree is organized by entity, never by host:

```
features/<entity>/               tasks, notes, focus, activity, history, downloads, resources...
                                 exports host-agnostic presentations: Row, Card, Editor, List,
                                 later Node (canvas). Props in, callbacks out; no host knowledge.
features/work/<sub-feature>/     ONLY what has no life outside Work: home, canvas (viewport
                                 adapter over @xyflow/svelte), plan, runs (activity, approvals,
                                 questions). Peers; never import each other.
app/work/WorkApp.svelte          composition root, lazily imported by app/browser/Shell.svelte.
domain/work/<slice>/             mirrors of the Work runtime projections, once the runtime
                                 (built separately, in Rust) exports them through zephium-ipc.
shared/ui/                       ONE kit. shared/ui/work is dead code (zero importers) and is
                                 deleted. Data primitives (Table, Chart behind a ChartSpec,
                                 Stat, KeyValue, Timeline) join the kit when blocks land.
```

Hosts are thin: sidebar slot, launcher panel, browser page, canvas node. `ToolSlot` already
takes `host: "sidebar" | "floating"`; that idea generalizes, the entity components never
branch on it. If a concept exists in Browse it is top-level and Work reuses it; no duplicates.

How Work uses an entity, in three parts:
- State: one projection per entity (`domain/tasks`), one store, one subscription. Both hosts
  read the same store; nothing is fetched or requested twice.
- Presentation: the entity feature owns the whole family, including the canvas shape
  (`TaskRow`, `TaskCard`, `TaskEditor`, `TaskNode`). "Tasks look different in Work" is a
  different export of the same feature over the same store, never a second implementation.
- Work semantics (assign to an agent, attach to a plan, mark as evidence) are Work state and
  live in `features/work/*`, which wrap the entity presentation and pass Work intents as
  callbacks.

Import rule, the single deliberate exception: `features/work/**` may import the public API
(`index.ts`) of top-level entity features. Entity features never import Work; no other feature
imports across features. The graph stays acyclic and the dependency is visible; the boundaries
lint encodes it as one rule. A registry assembled in `app/work` was considered and rejected as
indirection that hides a real dependency. `app/work` remains the composition root.

The Work runtime is being built by another agent in Rust. The frame defines no Work contracts.
The only coordination point is mechanical: runtime projections and intents must go through
`zephium-ipc` with specta types like every other slice; the frame mirrors them in `domain/work`
when they exist. Heavy libraries (XY Flow, ECharts, CodeMirror, Tiptap) are each their own
dynamic chunk loaded per representation on demand, never at Work entry, each with a budget row.

Nothing is built now. Recorded as ADR `docs/adr/0001-work-hosting.md`; the surface model
reserves `kind: "work"`. A Task is persistent unfinished user work; a plan node in Work may
reference a Task but is a different identity. Entities are never duplicated into Work state.

### 8.1 Entities are prepared, not built

Tasks and Notes are real product units (Notes is a knowledge base and agent memory, Tasks is
the unit of work), and they will be built by a dedicated agent later, end to end, in Rust and
frame. This track prepares the places and the contracts they must satisfy and writes no
entity code:

- Ownership, stated once: projection stores and IPC live in `domain/tasks` and
  `domain/notes` (one store, one subscription, typed intents). `features/tasks` and
  `features/notes` own presentation, interaction and temporary editor state and consume the
  domain public API; they never mirror Rust state themselves or open a second IPC path.
  Exports: `Row`, `Card`, `Editor`, `List`, later `Node`; props in, callbacks out; no host
  knowledge.
- No placeholder folders or READMEs are created. The entity contract above is a section in
  `docs/frontend.md`; folders appear when the entity agent starts.
- Drafts in entity editors are user data and autosave to Rust; the frame-only
  `session/tool-drafts` cap is for the current placeholder tools only and is not a pattern
  for real entities.
- Any interim data is a development fixture under `shared/testing/fixtures`, excluded from
  production by the build guard (product-system rule: no fabricated data in production).
- The Rust side of an entity is an ordinary slice (core aggregate, store table, projection,
  intents) exactly like items; that is the pattern the entity agent follows, not something
  this track pre-writes.

### 8.2 Time is not a tool; Focus and Activity are

The current "time" tool is a misread. The product needs two related units that share data:

- `features/activity/`: where time goes. Per-site and per-space time, visits, trends. Rust
  computes it from the visit history it already records; the frame renders it with the same
  data primitives as blocks (Table, Chart, Stat). This is the "mini dashboard in the browser"
  and its full page destination.
- `features/focus/`: control. Focus sessions, block rules (no Twitter while working), current
  session state. Enforcement is Rust: the navigation policy in core already gates URLs; a
  focus rule is one more input to it, never a frame check.

One sidebar tool, Focus, shows the session control plus today's summary; Activity is the page
destination. Rust: two slices (`activity` read model over history, `focus` aggregate). Both are
later work by the entity agent; `ToolKind::Time` is renamed to `Focus` in the IPC phase.
Naming: "Analytics" is rejected because in a zero-telemetry browser it reads as tracking;
"Activity" is the neutral Screen Time vocabulary. Work's run feed is therefore named `runs`
and never "activity".

### 8.3 AI in Browse: Ask mode in the launcher, one runtime

No standalone chat tool: it would compete with Work for the agentic identity. There is one AI
runtime (Rust, built by the runtime agent); the frame never has an "AI feature". Browse and
Work differ only in the mode the runtime is entered in:

| | Ask (Browse, launcher route) | Work (canvas) |
| --- | --- | --- |
| Lifetime | one session while the panel is open; discarded on close, no history list | persistent Work aggregate |
| Context | current page, selection, open tabs; read-only | attached resources, plan, agents, tools |
| Can act | no; may propose registry commands the user runs through normal admission | executes visibly under approval |
| Output | blocks (text, table, chart, list, resource links) | blocks plus Work state |
| Exits | dismiss; Save as note; Continue in Work (seeds a Work with question + context) | return to Browse |

Ask has no execution surface, so it cannot drift into a second agent. Proposed commands reuse
the launcher's existing command results and the commands mirror (3.3); no new frame authority.
Tree: `features/launcher/ask/` (a launcher route, not a tool), `domain/ask` mirroring the
runtime's session projection (idle, streaming, answered, failed, proposals) once exported.
Streams are coalesced in Rust before projection. `ToolKind::Ai` is removed in the IPC phase.
Disabling AI removes the route; Browse loses nothing.

### 8.4 Artifacts, notes and memory are three things; one renderer is shared

The agent never generates markup. It says what to create and where; the runtime holds the
state; the frame renders a projection. Three distinct product concepts, never one schema:

- `ArtifactBlockSpec`: Rust-owned, versioned native visual output generated by agents (table,
  chart, stat, list, resource reference, text). Rendered by `shared/blocks`.
- `NoteDocument`: explicit user knowledge, constrained versioned ProseMirror JSON for the
  Tiptap editor with a limited schema. A note may embed artifact or resource references,
  rendered through `shared/blocks` by an explicit adapter; it does not share the artifact
  schema.
- System memory: mostly invisible, privacy-conscious retrieval context. Never a frame concern.

```
shared/ui/data/*        spec-agnostic primitives: Table, Chart (ChartSpec), Stat, KeyValue,
                        Timeline. Plain props. Used by Activity, artifacts, canvas alike.
shared/blocks/          renderer for ArtifactBlockSpec: spec in, DOM out; interactive blocks
                        receive callbacks from the host. May import types from shared/ipc;
                        no store, no IPC calls, no domain imports.
features/work/canvas    node host: wraps an artifact or an entity presentation in a node.
features/launcher/ask   renders an answer (artifact blocks) with shared/blocks.
features/notes          edit mode is the Tiptap editor over NoteDocument; embedded references
                        render through shared/blocks via the notes adapter.
```

`ArtifactBlockSpec` and `NoteDocument` are defined in Rust as part of the joint contract (step
4). This track writes neither and ships nothing under `shared/blocks` until they exist.
Activity is the first consumer of `shared/ui/data` and needs no agent schema.

---

## 9. Launcher and always-on surfaces

Already sound; make the guarantees explicit and gated:
- Hidden panel = no feature subscriptions, timers, polling or workers. A small documented
  set of host lifecycle listeners stays (panel state, appearance, the listener that shows it
  again). The search controller and tool views already dispose.
- Panel entry budget row (section 7) fails the build when the panel graph exceeds its size
  budget or pulls a forbidden module.
- Placeholder tool drafts stay in `session/tool-drafts` with the existing 24-entry cap
  (not a pattern for real entities, see 8.1).

---

## 10. Documentation and contributor surface

- `docs/frontend.md` rewritten to this contract: layers, ownership principle, store shape,
  geometry rule, styling system (dated decision section with rationale), multi-entry surfaces
  (dated decision section), test placement, surface hosting rule, IPC projections table,
  localization contract, entity contract (8.1). It stays the single frame contract;
  `docs/design/system.md` and `interface.md` are reconciled to it (remove the stale `state/`,
  `src/ui/`, Studio references). No other frame doc is authoritative.
- One ADR only: `docs/adr/0001-work-hosting.md` (section 8 wording). Styling and multi-entry
  are decision sections in `frontend.md`, not ADRs.
- Existing agentic-browser and Work runtime documents are never deleted or rewritten by this
  track; `docs/README.md` stays the routing map.
- `frame/README.md`: one screen: layers, where things go, how to add a feature / a domain
  projection / a primitive / a surface, and the check commands. Linked from CONTRIBUTING.
- `docs/design/tokens.md`: the semantic token contract (name, meaning, which axes override it).

## Release targets

Windows and macOS are the launch platforms. Linux keeps compiling and every platform seam
stays cross-platform, but this track does no Linux runtime acceptance and runs no Linux
component tests. Component tests use WebKit on macOS and Chromium on Windows, the shipping
engines.

## What we are NOT doing

- No state library (no Redux-style store, no signals lib); runes stores + projections are the
  model and they work.
- No shadcn-svelte or component library swap; no Tailwind utility-first styling.
- No visual/design changes, with two explicit product-owner exceptions that are user-visible:
  the AI chat placeholder tool is removed (8.3) and the Time placeholder is renamed Focus
  (8.2). No Work, tasks, notes, focus, activity, blocks or theme-editor implementation. No
  placeholder folders or READMEs for them; existing tool shells stay in `features/tools/`
  marked temporary.
- No Work contracts, no edits to Work runtime crates or their docs.
- No account layer: not designed, not reserved, not touched. When it comes it follows the same
  pattern (projection in `domain/`, form in `features/`, tokens never leave Rust).
- No horizontal-tab layout UI; the model is mode-agnostic, the build is vertical.
- No change to presentation-barrier rules, CSP, capability split, or native layout invariants.
- No browser-automation e2e in the local gate (component tests only, per frontend.md).

---

## Commit discipline

One commit per logical change, never one commit per phase: a tool added, a module moved, a
projection typed, one CSS file migrated. Each commit compiles and passes the frame check on its
own. Rust and frame edits that depend on each other (a moved file and its `frame_sources.rs`
line, a new event and its listener) land in the same commit. Terse Conventional Commits, no
body unless the diff cannot show why, plain ASCII, no attribution trailers.

## Steps (each phase ends green on `pnpm -C frame check` and `cargo xtask ci`)

0. [x] Preserve and triage the working tree. Owner instruction, 2026-09-11: no
       baseline commits. Save a verified local archive including tracked/untracked
       source files, the original index, HEAD and binary diffs; do not switch branches
       or use the shared stash. Review by concern: retain / refine / supersede; no
       wholesale deletion. Existing icons remain separate; the retained logo belongs
       in frame/public. Implementation and snapshot details are in the progress log.
       Future commits remain coherent verified changes spanning Rust and frame where
       they depend on each other; none are made during this initial working pass.
1. [ ] Tooling: `frame/vitest.config.ts` (two projects), path aliases, `eslint-plugin-boundaries`
       + `no-restricted-imports`, `knip`, `stylelint`, `shared/testing/` mock factory. Colocate
       and rename the 24 tests, migrate them to the mock factory, delete `frame/tests/`.
       Verify: `pnpm -C frame check`, `pnpm -C frame test:component`.
2. [ ] Rust `desktop/src/frame_sources.rs` manifest; re-anchor every `include_str!` markup test.
       Move the CSS tonal gate to `cargo xtask check-frame-styles` over `frame/dist`.
       Verify: `cargo xtask ci`.
3. [ ] Restructure: create `session/`, purify `domain/`, split `features/sidebar` by concept,
       keep the placeholder tool views in `features/tools/` marked temporary (they are NOT
       moved into entity folders; see 8.1), delete `shared/ui/work`,
       add `index.ts` public APIs, `shared/lib/lifecycle.ts`, `domain/operations/settle.ts`. Pure moves
       and extractions, no behavior change; `frame_sources.rs` updated in the same commits.
       Verify: `pnpm -C frame check` (boundaries lint now enforces the graph), `cargo xtask ci`.
4a. [ ] IPC, ungated: scoped projections per 3.1 (`AppearanceState`, `PreferencesState`,
       `PrimarySurfaceState` with `browse` and `page` only, `CommandsState`); the layout
       contract and `chromeSetExtent(extent, seq)` echo (section 4); `PreferenceKey` typed
       keys; locale preference; `ToolKind`: remove `Ai`, rename `Time` to `Focus`; commands
       mirror with `CommandId` + `LocalizedText` and the Rust accessor build step from
       `frame/messages/`; `domain/appearance` as the only `<html data-*>` writer; delete the
       18 string parsers and the readback poll. Regenerate `bindings.ts` (`cargo test -p
       zephium-desktop export_typescript_bindings`). Verify: bindings drift gate,
       message-catalog drift gate, app tests, frame check. Step 5 depends on this step.
4b. [ ] IPC, GATED on the joint Rust/frontend contract (top-level surface state incl. the
       Work variant semantics, `WorkId` and revisions, Work entities and typed intents,
       `ArtifactBlockSpec` vs `NoteDocument`, error and lifecycle semantics): add the `work`
       variant to `PrimarySurfaceState` and `domain/surface`. The implementing agent stops
       and reports if the contract is missing; nothing else waits on it.
5. [ ] Surface model: `session/surface.svelte.ts` + `app/browser/shortcuts.ts` + the
       Preview/Applied/Committed geometry rule; delete `domain/shell/`. Verify: unit tests for
       surface transitions (tool while page open, escape/back, extent publish, commit-by-sequence,
       cancel adopts applied), manual smoke: settings/history/tools/compact mode/sidebar drag.
6. [ ] Styles: token tiers + axes, `base.css`/`material.css`, migrate `browser.css`,
       `components.css`, `panel.css` into scoped blocks; primitives to one-folder-each;
       split Switch/Checkbox, remove `Dropdown.svelte`. Verify: stylelint, built-CSS gate,
       visual parity screenshots dark/light on macOS (record OS in the PR).
7. [ ] Multi-entry: `browser.html`/`panel.html`, `entries/`, per-entry and per-chunk size
       ratchets + forbidden modules in `bootstrap-report.ts`, Rust window URLs, error
       boundaries around every lazy surface. Verify: `pnpm -C frame build`, panel opens with
       styles, `dist/bootstrap-report.json` per entry.
8. [ ] Docs and closure: `frontend.md` rewrite (incl. entity contract and localization
       contract), the one ADR (distinguishing the chosen default from measurements actually
       collected, with the measurement table filled or marked pending), `frame/README.md`,
       `tokens.md`, reconcile design docs; verify no lint override from step 1 remains.
       Documentation is updated with each change along the way; this step reconciles it.

## Verification (final)

```
pnpm -C frame check            # typecheck, eslint (boundaries), stylelint, prettier, knip, unit tests
pnpm -C frame test:component   # vitest browser mode (webkit on macOS)
pnpm -C frame build            # per-entry budget report must pass
cargo xtask ci                 # rust gates incl. bindings drift and check-frame-styles
pnpm dev                       # manual: browse, NTP, settings, history, tools, compact mode,
                               # launcher open/close x20 with Activity Monitor: no timers, RSS stable
```
