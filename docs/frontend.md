# Frame (frontend) handoff

Read this before touching `frame/`. It is the working contract for UI work;
architecture.md covers the system, security-model.md the trust rules.

## Stack (fixed, do not swap)

- SolidJS + TypeScript + Vite, Tailwind v4 (token-first), Kobalte for headless
  primitives (menus, dialogs, tooltips, popovers that stay INSIDE the chrome).
- Icons: hugeicons via `@hugeicons/core-free-icons` rendered by
  `src/ui/Icon.tsx` (our Solid renderer). No other icon packs.
- Animations: CSS-first (transform/opacity only). `solid-motionone` for
  orchestrated sequences (springs, staggers). Respect
  `prefers-reduced-motion`. Never animate layout properties.
- Fonts: Geist Variable (`--font-sans`), Geist Mono Variable (`--font-mono`),
  bundled via fontsource, imported in `src/index.tsx`.
- Lint/format: Biome (`pnpm lint`, `pnpm format`); `pnpm run check` = tsc +
  biome and is the gate CI runs. Generated `src/ipc/bindings.ts` and
  `src/styles/**` are excluded from biome.

## Hard rules

1. **Colors only through tokens** (`bg-*`, `text-*` utilities backed by
   `--color-*` vars in `src/styles/global.css`). Never raw hex, never
   `white/10`. Both themes must work: check `[data-theme="light"]`.
2. **The chrome cannot draw over web content.** The sidebar webview ends at
   its rectangle; anything over the page is native (menus, panel) - ask the
   core session, do not fake it with DOM.
3. **Native first for menu-shaped UI**: context menus and dropdowns go
   through `commands.menuPopup`-style native popups, not DOM. DOM popovers
   are fine only fully inside the sidebar.
4. **No authoritative state in the frame.** State comes from events
   (`itemsChanged` snapshot + `tabChanged` row delta, see `state/tabs.ts`);
   intents go through generated `commands.*`. Never invent local truth.
5. **`src/ipc/bindings.ts` is generated** (by `cargo test -p zephium-desktop
   export_typescript_bindings`). Never edit by hand.
6. Window label routing lives in `app/App.tsx`: `main` = chrome, `panel` =
   launcher overlay. On macOS it is currently a configured auxiliary NSWindow,
   not an allocation-time NSPanel; Esc hides via `commands.panelHide`, blur
   hides natively. Do not reintroduce live Objective-C class replacement.
7. `ui/FavIcon.tsx` accepts only the `rgba32:` fixed-raster value produced by
   Rust and paints it with `ImageData` on a canvas. Do not turn this into an
   `<img>`, data URL, custom protocol, or privileged image-format decoder.
8. Initialize `state/operations.ts` before normal mutation traffic. Accepted
   mutations are process-local ids with later typed dispositions, not immediate
   success. Keep the listen-then-reconcile order, idempotent result handling,
   and acknowledgement retry; do not describe this ledger as surviving process
   death.

## Existing patterns to follow

- `state/*.ts` - one store per domain, `init()/dispose()` pair, listen via
  `events.*`.
- `state/operations.ts` - process-local disposition reconciliation and explicit
  backend acknowledgement. Profile deletion is the only currently journaled
   cross-restart operation.
9. The main native window starts hidden on a hardened `about:blank`. The exact
   trusted app document must finish and call `commands.uiReady()` only after
   theme/material initialization and a synchronous style/layout flush. Keep the
   inline opaque bootstrap colors byte-exact with Rust's native presentation
   background. Never wait for `requestAnimationFrame` while the native window is
   hidden; engines may suspend it and deadlock startup. The native 15-second
   watchdog intentionally exits instead of displaying partial privileged chrome.
- `ui/*.tsx` - shared primitives (FavIcon, Icon). New shared components go
  here; feature-specific ones live in their feature folder.
- Keyboard: global shortcuts are native (menu/accelerators); chrome-local
  keys go through the `CHROME_KEYS` table in `App.tsx` and dispatch
  `commands.runCommand(id)` - command ids come from the Rust registry.
- Theme: `state/theme.ts` owns `data-theme`/`data-material`; appearance is
  persisted Rust-side (`setting_get/set`, allowlisted keys).

## Known debts left intentionally for the component pass

- NavButton/tab row/launcher row should become proper shared components with
  focus-visible states and roving tab index in the tab list.
- Kobalte is installed but unused; adopt it per-component (tooltip first).
