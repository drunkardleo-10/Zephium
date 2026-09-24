# File workflows and current UI integration

Integration checkpoint date: 2026-09-24. The subsequently committed hardening and
Windows candidate are recorded in [file-workflows-hardening.md](file-workflows-hardening.md).
The validation below describes the integration checkpoint. Branch: `feature/file-workflows-integration`.

## Lineage and scope

- UI/Tasks baseline: `d593bfc5` on `feature/ui-design-system`.
- Standalone macOS file-workflow checkpoint: `8bc96fc4` on
  `feature/browser-file-workflows`, originally based on `35308e16`.
- This normal merge preserves both histories. The original worktrees and UI
  branch are retained; nothing is pushed. Windows work now follows this checkpoint in the same isolated
  worktree; local-document viewing and Linux remain deferred.

## Reconciliation

- Retain profile migrations 1–19 byte-for-byte from the UI lineage. Append the
  download tables as migration 20, fingerprint `0x4b37b9cf91e8b507`.
- Regression tests verify that upgrading a profile with task data, lists,
  deadlines, estimates, resource usage and receipts preserves its data, and
  that the old standalone download QA version-16 schema is rejected before DML.
- Retain the newer Tasks models and boxed resource command. Regenerate bindings
  from the combined Rust source rather than resolving generated code by hand.
- Retain the newer ToolFrame API; Downloads uses `caption={false}`. Preserve
  address-field motion and native page motion while carrying the empty-tab
  navigation repair and file-picker/download lifetime guards.
- The full Downloads list is lazy. The small status indicator does not eagerly
  import its list, actions or list CSS. The indicator uses the UI's row radius.
- A behavior-equivalent close-guard predicate simplification resolves the strict
  Clippy warning present in the UI baseline (`resource_close.rs`).

## Bundle measurements and budgets

The combined build retains the 24-request browser/panel startup gate and every
existing Tasks JavaScript budget. Translation chunks follow their actual entry
consumers. Only common primitives, native IPC, the translation runtime and the
small shared motion-token helper are consolidated; list/plate animation modules
and feature implementations retain their own dependency boundaries.

| Static graph | JS bytes | CSS bytes |
| --- | ---: | ---: |
| Browser (24 HTML startup assets) | 331557 | 86820 |
| Panel (11 HTML startup assets) | 140589 | 27052 |
| Lazy Downloads list | 149356 | 5511 |
| Task board | 325562 | 35353 |
| Tasks | 316547 | 30217 |
| ToolSlot | 139620 | 4332 |

The new Downloads list has an explicit 155000-byte JS / 7000-byte CSS ceiling.
The download library retains the file-workflow checkpoint's 9000-byte CSS limit.
Three existing static graph CSS ceilings are adjusted for the same 3159-byte
shared button stylesheet, already loaded by startup: ToolSlot 3600 -> 4500,
Tasks capture 1500 -> 4200, resources index 3300 -> 4200. Their measured CSS
is 4332 / 3963 / 3963 bytes respectively. These are full dependency graph
measurements, not additional stylesheet downloads when opening those surfaces.
The unchanged UI snapshot was built separately to compare graph accounting.

## Validation

- Frame checks pass: types, lint, styles, formatting, boundaries, 248 unit tests.
- WebKit component suite passes: 141 tests across 42 files, including the new
  Downloads native-state/action test and existing Tasks/shell interactions.
- Core 369, app 352 and engine 383 tests passed. Store passed 290 tests with 2
  ignored in the combined serial run; a subsequent focused migration run passed
  49 tests with 1 ignored, including both new integration regressions.
- Desktop: 105 passed, 2 ignored. Generated bindings exported successfully.
- Targeted all-targets Clippy passes with warnings denied. One Store timing test
  timed out during the concurrent build/browser-test run; the complete combined
  Rust run passed with test threads serialized. No timeout assertion was weakened.
- Production frontend build and emitted CSS gate pass. The Windows core/engine
  cross-check passes (the three existing vendored dead-code warnings remain);
  this is compilation evidence, not a Windows download implementation. Native
  adapter lock/provenance checks pass.
- Full `cargo xtask ci` still stops at the previously reproduced extension
  acquisition source-inventory error for `desktop/foreground_probe_admission.rs`.
  The failing scanner and referenced sources are unchanged by this integration.
  The existing Svelte warning in `session/motion.svelte.ts` also remains visible.

## QA isolation and remaining product work

The integration build uses `app.zephium.files-integration-qa` / **Zephium Files
Integration QA**, restricted at this checkpoint to debug macOS and an exact bundled config. The
subsequent hardening also permits the isolated Windows debug QA configuration.
It does not reuse either `app.zephium.files-qa` or `app.zephium.resources-qa`.
Do not relabel an older standalone QA database's schema version to open it here.

The [standalone file-workflow record](file-workflows-progress.md) contains the
September 22 native evidence and remaining hardening work. This merge is not a
claim of complete release qualification. In particular, history retention must
preserve pending staging-cleanup receipts, recovery should not depend on opening
history, and native failure/teardown/drag/cloud-provider checks remain. Windows
returned Unsupported at this checkpoint; the subsequent adapter and remaining
qualification are tracked in the hardening record.

## Integrated native smoke

The unsigned integration bundle built and launched on this Mac with its fresh
profile. The current dock/tool selector opened the lazy Downloads panel; its
layout was visually inspected alongside the native website view.

- Native attachment download saved to
  `/private/tmp/zephium-integration-fixtures/downloads/zephium-download.txt`.
  UI showed Completed, 32 B / 32 B. The file exactly matched the generated bytes,
  SHA-256 `af816d943b8b840356cccd91cd2540b50d3c07d2f5b05c7fa128b77e55b4d559`,
  and retained a `com.apple.quarantine` attribute (`0281;...`).
- With Downloads still open, the website's native upload picker selected only
  the generated fixture. The loopback server verified 43 exact bytes including
  the embedded NUL, SHA-256
  `9afcbed43ee9f711a1f0e82908005cca1291eb57f6e86aed44769638aa50271c`.
- Switched to Tasks through the same dock and created the disposable task
  "Verify downloads integration". After quitting and relaunching, the task
  remained and switching to Downloads still showed the completed attachment.
- No normal profile, older standalone QA profile or personal upload file was used.

For this run, the generated-file fixture was
`http://127.0.0.1:64547/9c55663ae7b7b871ae2aad1d083938c528ca` (process-local).
The integration bundle is under the reused Cargo target directory:
`/private/tmp/zephium-browser-file-workflows/target/debug/bundle/macos/Zephium Files Integration QA.app`.
Its sources are in `/private/tmp/zephium-files-integration`, not the old download
worktree. Reusing Cargo's build cache does not change that old branch's source.

## Dropdown input-lock follow-up

The user reported that dropdowns left the app unusable and logged a style CSP
violation. The inline startup style blocks and CSP configuration were unchanged
from the committed UI baseline. Tauri injects a nonce into each inline HTML style
block and adds that nonce to the effective `style-src`; browsers then ignore that
directive's `unsafe-inline`. Bits UI sets body pointer events through CSSOM while
opening a dropdown, then restores the previous `style` attribute on close. The
restoration was blocked, leaving `pointer-events: none` on the body.

Startup paint now lives in `frame/src/styles/axes/bootstrap.css`, imported into
both existing root stylesheets. There are no inline style blocks in either emitted
privileged HTML document. The CSP configuration, script restrictions and Tauri
CSP modification remain unchanged. Native tests still verify exact canvas colors,
and the production build rejects a reintroduced inline style block.

A WebKit regression derives the effective style policy from the real CSP config
and both HTML sources. Before the fix, menu selection left pointer events at
`none`; after the fix, Menu and Select selection, Escape dismissal, subsequent
physical clicks and the absence of style-policy violations all pass. This covers
the interaction missing from the earlier native smoke.

Validation: 248 frontend unit tests, 142 WebKit component tests, 106 desktop tests
(2 ignored), frontend build and emitted CSS checks pass. The rebuilt native QA app
was relaunched; Tools and Tasks menus were exercised, and a physical-coordinate
click focused the task search field after selection and Escape dismissal.

References: [Tauri CSP processing](https://v2.tauri.app/reference/config/),
[nonce/hash precedence in CSP](https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/CSP).
