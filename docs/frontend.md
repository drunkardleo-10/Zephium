# Frame (frontend) handoff

Read this before touching `frame/`. It is the working contract for UI work;
`architecture.md` covers the system and `security-model.md` the trust rules.

## Stack (fixed, do not swap)

- Svelte 5 runes + strict TypeScript + Vite 8.
- Tailwind CSS v4, token-first. Bits UI supplies narrowly adopted accessible
  behavior; Zephium owns every visual component.
- Icons come only from the MIT/free packages `@hugeicons/svelte` and
  `@hugeicons/core-free-icons`, wrapped by `src/ui/Icon.svelte`.
- Geist Variable and Geist Mono Variable are bundled through Fontsource and
  imported in `src/main.ts`.
- ESLint with the Svelte and TypeScript configs, plus Prettier with the Svelte
  and Tailwind plugins. `pnpm run check` is the frame gate: Svelte/TypeScript
  diagnostics, lint, formatting, then Vitest.
- Vitest covers framework-free state and the small security-sensitive Svelte
  seams. Browser automation is intentionally not part of the foundation gate.

Use platform-native controls and surfaces whenever they fit. Bits UI is not a
default component catalog: add a primitive only when native HTML cannot provide
the required behavior. Popovers must remain inside their owning chrome WebView.

## Visual system

- Premium and quiet, informed by Arc and Zen rather than copied from either.
  Hierarchy comes from native material, spacing, typography, borders, and
  restrained neutral surfaces. No gradients, glow, fake glass, or decorative
  color noise.
- Colors and shadows come only through semantic tokens in
  `src/styles/global.css`. Never use raw palette utilities such as `white/10` in
  component markup. Every component must work in dark, light, reduced-motion,
  and forced-color modes.
- Use the 8 px spacing rhythm, compact 28–36 px chrome controls, and clear
  focus-visible states. Animate only transform/opacity or a narrowly justified
  color change; never animate browser geometry or startup.
- Customization is token- and domain-driven. Do not spread user preference
  conditionals through components or turn transient component state into a
  second settings system.

## Trust and native-boundary rules

1. **The frame is a projection, never authority.** Domain events update stores;
   user intent goes through generated `commands.*`. Do not optimistically claim
   native or durable success.
2. **The chrome cannot draw over page content.** Its WebView ends at the
   configured rectangle. Menus, context menus, and page-overlapping surfaces
   are native. A DOM popover is allowed only when fully contained in the
   sidebar; do not portal one across a WebView boundary.
3. **`src/ipc/bindings.ts` is generated** by `cargo test -p zephium-desktop
   export_typescript_bindings`. Never edit it by hand.
4. **Favicons stay fixed raster.** `ui/FavIcon.svelte` accepts only the bounded
   `rgba32:` payload produced by Rust and paints a 32 × 32 `ImageData` canvas.
   Never replace this with `<img>`, a data URL, a custom protocol, or a
   privileged image-format decoder.
5. **Operation settlement stays exact.** `state/operations.ts` listens before
   reconciling, deduplicates process-local dispositions, and retries native
   acknowledgements. Accepted admission is not success, and the ledger does not
   survive process death.
6. **Production CSP is a release invariant.** `script-src` remains exactly
   `'self'`; production must never gain `unsafe-eval`, inline script, `data:`, or
   `blob:`. Svelte production output needs none of them. The current inline
   bootstrap and dynamic native geometry still require style `unsafe-inline`;
   changing that is a separate hardening project, not migration cleanup.

## Native presentation barrier

Rust dispatches `zephium:presentation-tab` and inspects committed DOM in the
same synchronous expression before revealing raw page content. This is a
security boundary, not a rendering optimization.

- `state/tabs.svelte.ts` installs all three scoped projection listeners before
  its first `await`, admits revisions through the framework-free
  `TabProjectionModel`, and publishes a presentation inside `flushSync`.
- `src/main.ts` calls `flushSync()` immediately after Svelte `mount()` so
  initial DOM and subscription work exist before bootstrap returns. Do not add
  an application re-entrancy guard; Svelte 5 supports nested synchronous
  flushing and a guard could reject a valid newer presentation.
- Keep exactly one shell with a direct
  `data-zephium-active-tab={active ?? ""}` binding.
- Render tab rows in an ID-keyed `{#each}` block. Every authoritative row
  directly binds `data-zephium-tab-id`, `data-zephium-tab-url`, and
  `data-zephium-projection-revision`.
- The label sentinel's sole child is `{tab.title}`. Do not add hidden text,
  icons, badges, or literal whitespace inside it.
- The address is a real `HTMLInputElement` with one-way `value={...}` and an
  explicit `input` handler. Accept native synthetic input; never gate it on
  `event.isTrusted` and never navigate from that handler.
- `data-zephium-new-tab` is conditional markup with no outro. Never leave a
  transitioning or hidden sentinel behind.
- Do not virtualize away an active/presenting row or create drag/responsive
  duplicates carrying any sentinel attribute.
- Never move a barrier field through `$effect`, `tick`, a microtask, an
  animation frame, or an awaited callback.

## Startup and surface lifecycle

- `app/App.svelte` routes by native window label: `main` renders browser chrome;
  `panel` renders the launcher. The launcher never calls `uiReady()`.
- Theme initialization applies the system mode and subscribes to theme commands
  before its first native query. The main surface installs projection listeners,
  resolves theme/material, synchronously forces style/layout, and only then
  calls `commands.uiReady()`.
- Keep the opaque bootstrap colors in `index.html` byte-exact with the native
  presentation background (`#1b1b1f` dark, `#f4f4f6` light). A hidden WebView
  may suspend `requestAnimationFrame`; startup must not depend on one. Native's
  15-second watchdog intentionally fails closed instead of revealing partial
  privileged chrome.
- Every domain store exposes an idempotent `init()/dispose()` pair. Runes stores
  use the `.svelte.ts` suffix; pure admission/reconciliation models stay normal
  `.ts` and are tested without a framework.

## Structure

```text
src/
  app/            composition root only: window routing and the main shell
  domain/         mirrors of authoritative Rust projections, one folder per
                  slice, each pairing a runes store with a framework-free model
    blocker/ operations/ runtime/ tabs/ theme/ ui-commands/
  features/       user-facing surfaces; a feature owns its components, its
                  local model and any state only it consumes
    launcher/
    newtab/
    sidebar/      address/ essentials/ footer/ header/ shield/ space/ tabs/
    split/        includes layout.svelte.ts, consumed by nothing else
  shared/         cross-cutting with no domain opinion
    ipc/          generated bindings and scoped native DOM events
    ui/           visual primitives
    platform.ts   host traits such as IS_MAC
  styles/         global semantic tokens and platform material rules
```

**Where does a store go?** With the feature that owns it. It graduates to
`domain/` only when it mirrors an authoritative Rust projection or when three
or more features consume it. `layout.svelte.ts` is feature-local because only
the split dividers read it; `tabs` is domain because the sidebar, the shell,
the new tab and the launcher all project from it.

A feature folder never imports from another feature folder. Shared behaviour
moves to `shared/`, shared state moves to `domain/`.

Several Rust tests `include_str!` these exact paths to assert security-relevant
markup. Moving a file under `src/` means updating `desktop/src/lib.rs` in the
same commit.

Global shortcuts remain native. The small set consumed by the chrome WebView is
matched in `App.svelte` and dispatched through the Rust Commands registry.
Theme/material attributes are owned by `state/theme.ts`; appearance remains
persisted and allowlisted on the Rust side.
