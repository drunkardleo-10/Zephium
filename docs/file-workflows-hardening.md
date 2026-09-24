# Website file workflows: hardening and Windows candidate

Date: 2026-09-24. Worktree: `/private/tmp/zephium-files-integration`.
Branch: `feature/file-workflows-integration`, based on `98ceece0` (dropdown fix)
and `684a8ce2` (UI/Tasks plus macOS file-workflow merge). The delivery is split
into four scoped commits: shared lifecycle/recovery, Windows implementation,
qualification tooling and this documentation. The local integration target is
`feature/ui-design-system` in `/Users/enigma/Dev/browser-ui`, using a fast-forward
merge that preserves the commits and authorship. Pushing remains with the user.
The pre-existing untracked `docs/browser-file-workflows.md` in the UI checkout
is outside this delivery and is preserved unchanged.

This is an implemented and locally tested candidate, not a claim that all release
qualification is complete. Windows execution still needs the user's Windows host.
Linux and local-document navigation were explicitly deferred.

## Commit checkpoint and integration

- `7687a752`: shared lifecycle, persistence and cleanup, including matching UI/tests.
- `9c7279e4`: native Windows downloads and host lifetime/protection integration.
- `2337bc74`: isolated cross-platform QA tooling and generated-file fixtures.
- Documentation is committed separately after these implementation checkpoints.

Both implementation stages were exported from their exact Git index and compiled
independently: macOS core/Store/engine all-targets, plus Windows core/engine
all-targets. The QA stage passed desktop all-targets compilation and fixture/config
checks. Windows Store compilation needs the Windows C SDK for bundled SQLite;
that native validation remains in the Windows test script. The implementation,
UI, generated bindings and QA files were checked byte-for-byte against the
previously tested candidate before delivery.

The UI baseline `d593bfc5` is already an ancestor of the integration history. The
local handoff uses `git merge --ff-only` after rechecking branch tips and preserving
unrelated working files. No squash, rebase, force update or push is part of this
handoff. Continue subsequent implementation in `/Users/enigma/Dev/browser-ui`.

## Changes

### Shared ownership and recovery

- Migration 21 (`0x3483179692c933a6`) adds a bounded `download_cleanup` journal,
  independent of history. Migrations 1–20 keep their existing fingerprints.
  Forget/history eviction cannot discard cleanup ownership. Only an exact
  filesystem receipt acknowledgement can remove an obligation.
- Startup enumerates registered and deletion-pending durable profiles. It cleans
  in 50-record pages with one recovery worker, without opening Downloads. Late
  terminal persistence requests a rescan; the coordinator stops its idle timer.
- Profile deletion retains internal cleanup/terminal-save access after the normal
  registry grant is revoked. A failed drain does not mark the profile retired and
  thereby allow a subsequent deletion attempt to bypass proof.
- Private cleanup has a separate, bounded, memory-only receipt map. Completion
  and removal from visible history do not discard that receipt. Orderly private
  profile/global shutdown drains it without persisting private paths.
- Explicit Cancelling state; late progress samples cannot change terminal Store
  acknowledgement revisions. Cancellation wins a race with native completion
  before publication, while finalization ignores late native failure callbacks.
- Destination decisions cancel on drop. Detached cleanup/persistence work also
  counts against admission, preventing repeated fast transfers from creating an
  unbounded backlog of slow filesystem workers. Existing bounds remain eight
  active transfers, four UI calls, 64 recent/private receipts, 10,000 durable
  history/cleanup records; new admission pauses at 32 outstanding work items.
- Portable filename handling covers reserved Windows device names, alternate
  streams, separators and bidi controls. Collision suffixes remain within 240
  UTF-8 bytes, preserving normal extensions and complete Unicode characters.

### Windows

- Human WebViews opt into a narrow Wry DownloadStarting hook. Explicit denial
  wins for privileged/agent contexts. WebView2 retains transport, cookies, POST
  bodies and blob/data ownership. No URL replay or second HTTP client is used.
- Native deferrals remain cancelled until a selected destination and durable
  staging receipt are acknowledged. Native COM file/folder dialogs, interruption
  mapping, cancellation, progress, Open and Reveal are implemented.
- Every Windows download requires native save confirmation. The event has no
  originating-frame activation/navigation identity; a simultaneous address-bar
  navigation cannot safely supply that proof. Settings explains the limit and
  disables automatic saving. A selected folder still seeds the dialog.
- Canonical ancestor handles, protected user/SYSTEM staging ACLs, reparse rejection,
  complete 128-bit file IDs, exclusive same-volume publication, Attachment Services
  and verified Internet-zone metadata protect the file boundary. A protection or
  unsupported-filesystem failure cannot be shown as Completed.
- Recovery refuses an open writer and retains uncertain native process ownership
  by PID plus creation time. Deletion addresses only the fixed payload and exact
  empty staging directory, never a recursive user-directory walk.
- Closed source tabs revoke page authority, hide/disable scripts and park at
  about:blank while a bounded retained controller owns the transfer. Shutdown
  joins download drain with existing verified process-group teardown.
- Native stopped/cancelled navigation is distinguished from controller failure,
  preserving an uncommitted download destination context without inventing a
  document commit or presentation permission.
- Origin-less data URLs expose only their page context in history, explicitly
  labelled as such; large data URL payloads are not copied into UI metadata.

### UI and qualification support

- Downloads shows Cancelling and actionable cleanup failures/retry while keeping
  history usable. A late history response cannot erase a newer cleanup failure.
- The status indicator takes one bounded native snapshot on subscribe, including
  after profile changes, then follows events without history or idle polling.
- Extended generated-only loopback fixtures cover empty, unknown-length and
  deliberately truncated responses plus a large data URL.
- Added an isolated Windows debug QA configuration and
  `scripts/qualification/build_file_workflows_windows.ps1`. The QA feature still
  rejects release builds and mixed QA identities. Windows instructions and the
  native acceptance matrix are in [file-workflows-windows-qa.md](file-workflows-windows-qa.md).

## Validation evidence

- Final combined Rust library run: core 371, engine 387, Store 275, desktop 106
  passed; 3 existing tests ignored. Includes cleanup after history removal and
  capacity eviction, deletion authorization, private receipt ownership, terminal
  races, progress reentrancy and filename bounds.
- Targeted all-targets Clippy passes with warnings denied.
- Frame checks pass: types, lint, boundaries, formatting, styles and 250 unit
  tests. WebKit component suite: 142 passed, including Cancelling actions and the
  packaged dropdown CSP regression. Production build/CSS/startup budgets pass;
  no budget was raised in this round. Browser static graph: 332469 JS / 86952 CSS
  bytes; panel: 140589 JS / 27184 CSS bytes.
- Windows engine all-targets cross-check and the vendored Wry Windows test-target
  cross-check pass. These do not execute Windows code. The same three pre-existing
  vendored permission dead-code warnings remain.
- Native adapter lock/provenance check passes. The existing full-CI acquisition
  scanner blocker is reproduced by `check-extension-runtime-acquisition-boundary`:
  `desktop/src/foreground_rendering_probe.rs` resolves
  `desktop/foreground_probe_admission.rs` outside its scanned inventory. That
  source-inventory defect predates this work; the gate was not bypassed.
- The pre-existing Svelte warning in `session/motion.svelte.ts` remains.

## Actual Mac app evidence

The isolated **Zephium Files Integration QA** bundle was rebuilt from this
worktree. Its profile migrated to 21 and retained earlier UI/Tasks/history data.
Using generated files and a tokenized loopback server:

- Attachment: Completed, exact 32 bytes, SHA-256
  `af816d943b8b840356cccd91cd2540b50d3c07d2f5b05c7fa128b77e55b4d559`,
  with `com.apple.quarantine`. No leftover stage or cleanup receipt.
- Truncated 1 MiB response: Failed, no published partial, zero staging directories
  and zero cleanup receipts after settlement.
- Native picker upload: server verified the exact 43-byte fixture including its
  NUL byte, SHA-256
  `9afcbed43ee9f711a1f0e82908005cca1291eb57f6e86aed44769638aa50271c`.
- Slow 32 MiB transfer: visible progress, cancelled at 8.1 MiB, Cancelled history,
  no published file or staging residue.
- Orderly quit/reopen succeeded. Completed, Failed and Cancelled history survived.
  Tools selection worked and a subsequent physical-coordinate chrome click
  closed Downloads, exercising actual pointer input after menu dismissal.

These tests cover the shared/native paths before the final admission-bound/private
bookkeeping refinement; the final source also passed the Rust checks and is rebuilt
for handoff and was relaunched successfully. The final executable SHA-256 is
`0d780b35b086e4349595d176451808d49825b15c02cde90b6bb76a8b87fafb51`.
These checks are not a forced-crash, release-signing or Windows runtime proof.
The previous broader macOS upload/POST/blob/tab-close checks are retained as dated
checkpoint evidence in [file-workflows-progress.md](file-workflows-progress.md).

## Remaining release work

1. Execute the Windows matrix on real Windows with the supported WebView2 floor
   and a current runtime, including protection, modal event ordering, tab close,
   profile deletion and teardown. The Windows script itself has not been executed
   here; no Windows installer was built on this Mac.
2. Run signed/package-level macOS and Windows checks for cloud-provider files,
   removable/network destinations, disk-full/read-only targets, forced process
   termination and repeated-workflow resource use. No endurance claim comes from
   a debug smoke test.
3. Resolve private crash cleanup deliberately. Private receipt paths are not
   persisted, so abrupt process death can leave an incomplete hidden staging
   directory in the selected destination. Orderly teardown is implemented; the
   privacy-preserving crash-recovery design remains open.
4. Fix the existing repository CI source-inventory blocker before release.

Automatic transfer resume remains unsupported; retry starts from the website so
POST/blob semantics cannot be silently replayed incorrectly. macOS native upload
accept-hint filtering is limited by the pinned public API; Windows uploads remain
engine-owned rather than a custom initiating-frame broker. Those limits are not
changed or hidden by this download implementation.

API references used during review:
[WebView2 download operation and interruption semantics](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2downloadoperation),
[Attachment Services filename contract](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iattachmentexecute-setfilename),
[WebView2 navigation error classification](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2navigationcompletedeventargs).
