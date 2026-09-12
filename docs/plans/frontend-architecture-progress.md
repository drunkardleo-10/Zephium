# Frontend foundation implementation progress

Updated: 2026-09-11. This records implementation and evidence, not a production
readiness declaration. The target remains [frontend-architecture.md](frontend-architecture.md).

## Preserved baseline and scope

No commits, branch changes, resets or shared stashes were made. A checksum-verified
archive of tracked and untracked source files, the index, HEAD and binary diffs is at
`/private/tmp/zephium-ui-baseline-20260911-143640`. Its source manifest covers 1,456 files;
ignored dependency/build directories remain outside the archive. The original index
is preserved and has not been changed by this migration.

Existing native material, launcher lifecycle, browser-page admission, preferences,
Settings compositions and their tests were retained. The unused Work fixture kit
was removed as planned. Placeholder tools remain under `features/tools`, not entity
folders. Existing desktop icon work remains separate. The owner's retained logo was
moved unchanged to `frame/public/zephium-logo.png`; visual placement is still later work.

## Implemented so far

- Tooling: shared Vite/Vitest aliases, separate unit/component projects, colocated
  tests, typed fail-on-unexpected-call IPC mocks, scoped event fixtures, strict
  explicit-any checking, import direction and public API enforcement, Stylelint,
  Knip, and macOS/Windows browser-component CI jobs.
- Architecture tests exercise forbidden upward/cross-feature/deep imports and
  generic Tauri event access. A style-policy test rejects literal component colors.
  WebKit tests exercise keyboard activation, focus, disabled/mixed states, exact
  tab-label sentinels and synchronous scoped event delivery.
- Native source anchors live in `desktop/src/frame_sources.rs`. Bootstrap-paint
  and presentation-barrier tests remain independent. `cargo xtask check-frame-styles`
  scans emitted CSS; `cargo xtask ci` invokes it after building the frame.
- Sidebar regions became product features: tabs, essentials, address, spaces,
  extensions and blocker. Shell supplies their snippets to the sidebar frame.
  The address field receives its blocker presentation as a snippet; the tab list
  receives the Essentials tile presentation. New Tab receives Settings preview
  values through composition props, not a cross-feature store import.
- Shared transient state lives in `session`; layout, appearance and browser-page
  mirrors live in `domain`. Feature/domain consumers use `index.ts` public APIs.
  Settings and floating-tool entry points expose lazy loaders. Neither ToolSlot nor
  its draft store is admitted to the panel startup graph (verified against emitted modules).
- `shared/lib/lifecycle.ts` provides generation guards and partial-listener cleanup.
  `domain/operations/settle.ts` consolidates admission/disposition waiting with a
  bounded admission timeout and a distinct deferred outcome. Existing callers still
  await their existing confirmation mechanism; the preference poll is not yet removed.
- Production bundle checks reject test/fixture/gallery code in every chunk.
  Tailwind source scanning is bounded to frontend source and excludes tests/fixtures.
  Documentation and the contributor entry point reflect the current migration.

## Verification

| Check                                                 | Result                                                                                   |
| ----------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `pnpm -C frame check`                                 | Passed: types, ESLint, Stylelint, formatting, Knip; 108 unit tests in 29 files           |
| `pnpm -C frame test:component`                        | Passed: 4 tests in 3 files, Playwright WebKit on macOS                                   |
| `pnpm -C frame build`                                 | Passed, including emitted graph and fixture guards                                       |
| `cargo xtask check-frame-styles`                      | Passed against emitted CSS                                                               |
| `cargo test -p zephium-desktop --lib`                 | Passed: 103 tests, 2 ignored                                                             |
| Dependency audit                                      | No known vulnerabilities after the Vitest 4.1.11 patch                                   |
| Rust formatting and dependency peers                  | Passed; no peer conflicts                                                                |
| Full `cargo xtask ci`                                 | Blocked before frontend checks by the existing runtime acquisition source-inventory gate |
| Windows component/native execution                    | Not run locally; component job added to CI                                               |
| Native visual, interaction and resource qualification | Pending                                                                                  |

The full workspace gate fails with:

```text
extension runtime acquisition boundary failed:
desktop/src/foreground_rendering_probe.rs external module resolves outside the
scanned Rust source inventory: desktop/foreground_probe_admission.rs
```

Both referenced files and the gate are present in unchanged HEAD. The inventory
scans `desktop/src` but omits root-level desktop Rust helpers. Investigation also
identified existing runtime files with production items after inline test modules,
and nested test modules, which the gate's current source-layout rules reject.
The gate and runtime sources are left unchanged; no exemption or weakened check
was shipped. This must be reconciled with the runtime track before claiming a green
whole-workspace gate. Focused frontend and desktop results above are independent
of that unresolved gate.

## Remaining work and next phase

Steps 1–3 have implementation and focused local evidence, but are not marked fully
qualified while the plan's whole-workspace phase gate remains red. Temporary
Knip/Stylelint debt inventories remain explicit and must be removed with the relevant
module/control migrations. They are not a final foundation configuration.

Next is step 4a: typed Browse projections, durable preference confirmation,
commands/localization, appearance ownership and the correlated layout protocol.
The existing store preference API only admits a queued write; emitting a new typed
snapshot from that admission would still be incorrect. Introduce truthful store
completion/reconciliation before deleting the frame's readback confirmation.

Then complete the unified surface model, scoped semantic styling, multi-entry
startup/budget ratchets, error boundaries, final documentation and native QA.
The current `session/sidebar-mode` and `session/tools` modules are transitional;
step 5 replaces their coordination with the single surface model.

Step 4b alone remains gated on the external Work contract. No Work entities,
agent runtime, account architecture or speculative Work IPC was implemented here.

## Nonvisual foundation pass — 2026-09-11

The design track owns components, blocks, styling, tokens, motion and layout.
This pass changed only state/lifecycle helpers, domain/session controllers, tests,
and this documentation. No component interfaces, UI files, motion implementation,
Rust contracts, commits or pushes were changed by this pass.

- Added bounded, abortable observation and retry-delay cleanup. Observation abort
  never sends a cancellation command to native code.
- Preserved operation identities on unresolved/timeout results and rejected ledger
  responses for another operation. Disposal now releases in-flight observation and
  reconciliation timers; already-issued native effects and acknowledgements retain
  their native semantics.
- Made preferences and browser-surface initialization single-flight. Preference
  startup reads cannot replace a newer event; write readback has a total deadline,
  cannot overwrite a newer event, and is invalidated on disposal/reinitialization.
  The existing readback confirmation remains pending the Rust projection migration.
- Added generation and request guards to existing tool transition callbacks, without
  changing animation timing or implementation. Closing invalidates queued tools;
  late subscriptions and callbacks cannot resurrect disposed state.
- Partial listener-registration failure now releases every successful sibling even
  if another unsubscribe throws.
- Verification: frame check passed with 130 unit tests in 33 files, production build
  and bundle budgets passed, and emitted CSS checks passed. Native/UI qualification
  was not claimed by these state tests. The previously documented unrelated full-CI
  issue remains separate; no runtime gates were bypassed or modified.

Next nonvisual work remains the typed Browse projection/confirmed-preference and
surface/layout contracts. Work components, blocks and final composition are deferred
until the design and nonvisual tracks are ready to meet, per the owner's instruction.

## Browser interaction corrections — 2026-09-12

- The local six-commit checkpoint was rebuilt into 36 concern-based commits using
  an alternate Git index. The replacement tip has exactly the same Git tree as
  the previous tip; every working file and the normal index were verified unchanged.
  The original history remains at `backup/ui-before-split-20260912`. No push occurred.
  This is a source-preserving history repair, not a claim that every intermediate
  commit independently builds. The copied Work contract remains untracked.
- `tab_drag_over` now accepts paired null coordinates as an explicit indicator
  clear. Ordinary clicks do not send a reset, and reset rejection is handled without
  leaking a failure into a subsequent gesture. Generated bindings were regenerated
  and already described nullable coordinates; the native signature was inconsistent.
- A reproduced Settings-return rejection compared a retained native single-leaf
  split with a public projection that intentionally omits it. Return now compares
  the native split tree with the captured native tree, retaining exact-state checks.
  A regression failed before the correction and passes afterward. A WebKit component
  test also verifies synchronous Settings-to-Browse tab/address restoration.
- Validation: 326 application tests and 103 desktop tests passed (2 desktop tests
  ignored); 13 WebKit component tests passed. This does not establish that the
  user's currently running binary contains these changes or qualify Windows behavior.
- The browser and Settings graphs that include drag state grow by 69 JavaScript
  bytes for the reset rejection/generation guard. Their limits receive 128 bytes
  of explicitly reviewed allowance; CSS limits and unrelated graphs stay unchanged.
  The production build passes with these limits.
- The deferred TanStack experiment was removed, retaining the existing lightweight
  table. The uncommitted XYFlow canvas remains a draft; its unused entry and the
  not-yet-used LayerChart dependency prevent the aggregate Knip check from passing.
  Finish the development entry, interaction tests, viewport restoration and measured
  lazy graph before treating that foundation as ready for handoff. Do not silence
  those findings through permanent exceptions or connect the draft to Browse.
