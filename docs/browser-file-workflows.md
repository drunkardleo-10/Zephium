# Browser file workflows: assessment and implementation plan

Assessment date: 2026-09-22. Source baseline: `feature/ui-design-system` at
`35308e16`, including the working tree inspected that day. This is a source/API
assessment, not a live reproduction or a claim of platform qualification.

## Finding

The reported silent failures have explicit causes in the native boundary.
Downloads, macOS/Linux upload pickers, and local-file navigation are deliberately
disabled. The browser needs complete file workflows, not only a Downloads view.

| Workflow | Current implementation | Evidence |
| --- | --- | --- |
| Website downloads | Raw views deny at construction; Shell ignores the existing URL-only event | `crates/zephium-engine/src/host/construction.rs:899`; `crates/zephium-app/src/shell/engine_events.rs:357` |
| macOS upload picker | Native completion receives no selected files | `vendor/wry/src/wkwebview/class/wry_web_view_ui_delegate.rs:220` |
| Linux upload picker | `run-file-chooser` is cancelled | `vendor/wry/src/webkitgtk/mod.rs:1128` |
| Windows upload picker | Repository documents engine-owned native selection and no supported interception hook in its current adapter | `docs/security-model.md:346`; requalify against the actual shipping SDK/runtime |
| Open a local file in a tab | Navigation permits HTTP(S) and exact `about:blank`; rejects file URLs and local paths | `crates/zephium-core/src/navigation.rs:51,106` |
| Downloads destination | Empty preview surface | `frame/src/features/tools/components/previews/DownloadsView.svelte` |
| Download preferences | Session preview values, not effective runtime policy | `frame/src/features/settings/components/sections/DownloadsPage.svelte` |
| Durable download records | No download subsystem found in Store | `crates/zephium-store/src` |

The deny policies also cover privileged browser WebViews and agent contexts.
Enabling a foreground human workflow must not implicitly authorize those other
principals. Ordinary upload selection grants access to selected files for that
request; it is not a persistent site-wide filesystem permission.

## Native feasibility and constraints

Native engines provide the foundation. Extend the maintained Wry adapter only
where native callbacks are otherwise inaccessible; keep product decisions in
Zephium rather than embedding them into the vendor layer.

| Platform | Upload path | Download path | Important constraint |
| --- | --- | --- | --- |
| macOS | WKUIDelegate open-panel completion and a browser-owned NSOpenPanel | WKDownload and its delegate/progress | Retain exact native callbacks/objects on the main thread; optional resume data is not a universal pause/resume guarantee |
| Windows | Existing engine-owned picker, subject to qualification | DownloadStarting deferral and ICoreWebView2DownloadOperation | Native operation supports progress, cancel, pause and conditional resume; do not invent picker interception where the pinned SDK has none |
| Linux | WebKitFileChooserRequest plus native chooser | WebKitDownload on WebContext | Route once per profile context to the actual originating view; the documented API has cancellation/progress but no equivalent pause/resume methods |

Apple documents the [upload panel completion contract](https://developer.apple.com/documentation/webkit/wkuidelegate/webview(_:runopenpanelwith:initiatedbyframe:completionhandler:))
and [multiple/directory selection parameters](https://developer.apple.com/documentation/webkit/wkopenpanelparameters).
Accept filters, initiating-frame identity, and user-activation evidence must be
mapped from what each native callback actually exposes. A page-supplied URL or a
recent generic click is not a substitute for missing native evidence.

[WKDownload](https://developer.apple.com/documentation/webkit/wkdownload) exposes
native download ownership and cancellation with optional resume data.
[WebView2 download operations](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2downloadoperation)
provide byte/state events and conditional resume. Its
[destination API](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2downloadstartingeventargs)
can defer a decision and can overwrite an existing destination: Zephium must
own collision policy.
[WebKitGTK downloads](https://webkitgtk.org/reference/webkit2gtk/stable/class.Download.html)
provide native progress and cancellation;
[asynchronous destination selection](https://webkitgtk.org/reference/webkit2gtk/2.42.4/signal.Download.decide-destination.html)
is supported from WebKitGTK 2.40, matching the crate's enabled API feature floor.
SDK API availability alone does not establish runtime support or behavior.

The current Wry start/completion callbacks are insufficient as the product API:
they lack stable transfer IDs, progress/control handles, deferred destination
ownership, and structured interruption reasons. macOS completion drops the
destination and failure resume data. The Linux fallback uses context-level
registration, and Zephium installs an additional deny handler there. Removing
only `.with_download_policy(DenyWithoutMetadata)` cannot deliver the feature.

## Ownership and contracts

- `zephium-core`: typed file-selection and download IDs, request identities,
  lifecycle states, capabilities, commands/events and port contracts.
- `zephium-engine`: native transfer/chooser handles, callback settlement,
  profile/view/navigation identity, progress observation and lifecycle leases.
- `zephium-app`: admission policy, bounded coordination, settings application,
  user-visible failures and projection. Start with a focused module; split a
  dedicated service crate only if supervision/ownership calls for it.
- `zephium-store`: versioned download history and actual preferences, paginated
  by profile. No durable private-session download history.
- `desktop`: window-parented native pickers, platform file provenance, explicit
  Open/Reveal actions and application shutdown integration.
- `frame`: typed projection into the existing Downloads surface and settings.
  It never owns file authority, native paths for arbitrary execution, or bytes.

For a pending chooser or destination prompt, bind the request to profile, tab,
physical view generation and document identity. Complete it exactly once on
selection, cancellation, navigation invalidation, close, crash or shutdown.
Serialize visible prompts and bound pending requests; do not reuse the media
broker's timeout blindly for a person browsing a large folder.

An admitted download needs a different lifetime: it belongs to the profile and
its stable DownloadId, and should survive ordinary navigation and logical tab
closure. First prove what native objects must remain alive on each engine.
Retain those through an explicit transfer lease, including in discard and
profile-erasure accounting. If survival cannot yet be implemented, surface a
cancel confirmation or explicit interruption; do not silently lose a transfer.

Use the native browser transfer rather than fetching its URL again with an
application HTTP client. This preserves the opportunity to handle authenticated
sessions, POST responses, redirects and page-generated downloads correctly.
Each still needs live qualification; native ownership does not prove every
`blob:`, `data:`, service-worker or export flow works through today's policy.
Do not widen the general navigation scheme allowlist to admit these downloads.

## Delivery sequence

### 1. Establish fixtures and restore website uploads

Build a small loopback fixture with single/multiple inputs, type hints, folder
selection, nested/cross-origin frames, drag/drop and a multipart echo endpoint.
Use harmless generated files. Click/type through the actual product to record
the baseline, including Windows' existing picker behavior.

Add an opt-in native file-selection contract with deny-by-default construction.
Implement macOS first, then Linux, keeping selected paths/completions native.
Windows should retain its engine-owned behavior unless a supported and needed
hook is proven. Request identity and unsupported capabilities must be explicit.
The first useful milestone is selecting a file, submitting it, and verifying
that the server receives the exact bytes; picker appearance alone is not done.

Do not add an upload manager: websites own upload progress after selection.
Folder upload, cloud-provider files, `accept` hints and drag/drop require their
own platform evidence. File System Access APIs are a separate capability.

### 2. Deliver one complete download flow

Introduce a native transfer adapter and Rust lifecycle model. Start with:
pending destination, transferring, finalizing, completed, cancelled, interrupted
and failed; make pause/resume per-transfer capabilities rather than universal
buttons. Store interrupted state after restart when native resumption is not
available. Retry means a new attempt, and cannot blindly replay a POST or an
expired/signed/blob URL.

The first production slice includes:

- Native transfers with stable IDs, bounded/coalesced progress and reliable
  terminal events. Unknown total size must remain indeterminate.
- Default Downloads folder and a real Ask Where to Save preference; native
  folder selection, persistence and clear handling of unavailable destinations.
- Safe suggested names, no path traversal, Windows device/alternate-stream
  names, invalid characters or uncontrolled overwrite. Collision handling must
  cover concurrent transfers and symlink/path substitution, not `exists()` alone.
- Private staging where compatible with the native engine, validated final
  publication and cleanup. Test cross-volume destinations and process death
  during publication; do not claim atomicity across volumes.
- Platform provenance before exposing a file as ready to open: verify macOS
  quarantine and Windows Mark-of-the-Web/native attachment policy on the actual
  produced file, including after moves/copies. Establish explicit behavior on
  filesystems that cannot retain the required metadata.
- Cancellation, disk-full, offline, permission-denied and server-error outcomes;
  partial-file cleanup and recovery that cannot delete unrelated files.
- Paginated history, Open, Reveal and missing-file handling. Clearing history
  and deleting downloaded files are distinct actions. Never auto-execute files.
- Profile isolation, private-session history behavior, tab-close/discard leases,
  profile deletion and a defined quit-during-download flow.
- Existing UI connected to real events and preferences, with accessible
  progress/status announcements and bounded automatic-download admission.

Apple's [quarantine configuration](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/LaunchServicesKeys.html)
and Windows [attachment services](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-iattachmentexecute)
are relevant platform contracts. Neither a filename heuristic nor a metadata
flag should be advertised as malware scanning. Preserve native security blocks;
identify any further protection claims separately.

### 3. Complete file entry points and local viewing

Add Save Link/Image As, generated exports and PDF save behavior through the same
download policy. Windows currently hides PDF Save/Save As/Print and disables
default context menus because some paths bypass DownloadStarting; audit those
paths before enabling them. Retain denial on unbrokered alternate save paths.

Implement Open File / Cmd-or-Ctrl+O as a separate user-authorized local-document
path. Begin with explicitly supported renderable types, an unprivileged viewer
and a visible local-file location. Define exact-file versus directory resource
access, relative subresources, navigation, session restoration and missing-file
behavior. Do not simply permit arbitrary `file:` URLs from remote pages or grant
local documents privileged chrome IPC. Opening a downloaded file in its default
OS application and viewing it in a browser tab are separate actions.

### 4. Track the remaining everyday-browser gaps independently

Source inspection also finds deliberately denied JavaScript dialogs, new-window
requests and authentication prompts; page permissions remain release-gated.
These can break confirmations, login flows and conferencing even after files
work. Audit them as subsequent origin-labelled native workflows. Also qualify
clipboard, find-in-page, print, external-app links and fullscreen. Some underlying
ports already exist (for example native print); do not label every item absent
without tracing its product command and testing it.

## Release evidence

| Area | Required evidence |
| --- | --- |
| Upload | Exact received bytes; single/multiple, cancellation, folder semantics, hints, frames, drag/drop, native/cloud-provider files |
| Download sources | Attachment GET, authenticated cookies, redirect, POST export, generated blob/data, service worker, unknown size, parallel same-name downloads |
| Filesystem | Existing destination, unsafe names, symlink substitution, disk full, unavailable directory, external volume, missing/moved completed file |
| Lifecycle | Navigate, close tab/window, discard, profile switch/delete, renderer death, quit, crash/restart during transfer and finalization |
| Trust | No prompt/transfer authority in privileged or ungranted agent views; stale responses rejected; no unsolicited bulk downloads; quarantine/MOTW retained |
| Performance | No startup history scan or idle polling; bounded progress queue; large files stream natively without whole-file IPC or JS buffering |
| Platforms | Signed packaged macOS behavior, actual Windows WebView2 and Linux Wayland/X11 behavior on the supported runtime floors |

Use unit tests for lifecycle transitions, filename/destination policy, request
identity and Store recovery. Use native integration fixtures for engine
contracts and real product interaction for picker, download and opening flows.
Static checks and a successful launch do not substitute for these gates.

## Branch and scope

The UI worktree contains substantial unrelated uncommitted work. Keep it intact.
Before implementation, create a sibling worktree on a dedicated branch such as
`feature/browser-file-workflows`, based on an explicitly checked committed
runtime baseline. The inspected UI HEAD and `extensions-public-install` have
diverged (82 commits versus 1); choosing the other branch without reviewing the
delta would discard relevant runtime work from the implementation baseline.
Do not stash or carry the user's UI edits into this branch implicitly.

Keep changes reviewable: native fixtures/contracts; upload adapters; download
adapters and destination safety; lifecycle/Store; UI wiring; local-file viewing.
Update `docs/security-model.md`, architecture, and vendor provenance alongside
each actual behavior change. This assessment adds documentation only and does
not change branches or enable a native capability.
