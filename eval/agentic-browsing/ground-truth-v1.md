# Agentic browsing ground truth v1

Reviewed 2026-08-31 against repository revision `6fcf29c`. This is evidence
for Milestone 0, not a claim that the target architecture already exists.

## Current ownership map

| Concern | Current implementation evidence | Consequence for agentic browsing |
| --- | --- | --- |
| Browser identity | `zephium-core/src/ids.rs` defines persistent `ItemId`; `Engine` operations in `zephium-core/src/ports/engine.rs` are keyed by `ItemId`. There is no `ContextId`. | Owned contexts cannot be projected as tabs. Milestone 2 needs a distinct port; a private temporary `ContextId` to `ItemId` adapter must not persist or escape. |
| Profile identity | `Partition::{Default,Persistent,Ephemeral}` binds an exact `ProfileId`. `host/profiles.rs` permanently binds each process-lifetime profile id to durable or ephemeral persistence. | Probe construction must always name a fresh ephemeral test profile. Production owned-context storage equivalence is unproved. |
| macOS data store | `platform/macos/mod.rs` creates and verifies non-persistent `WKWebsiteDataStore` objects for ephemeral profiles and creates a fresh configuration per view. | The M1 probe can use an isolated ephemeral store. Exact selected durable profile plus extension-free configuration remains Milestone 2 work. |
| Windows profile/process lifetime | `host/profiles.rs` admits at most eight native profile process groups and retains Environment5 exit proof. | A future extension-free `agent-<profile-id>` subprofile must enter the same bounded lifecycle; no extra environment may bypass it. |
| Extensions | Production native extension state is profile-scoped and separately owned. Existing probe-only extension code is feature-gated. | The agentic input probe does not reuse or modify extension-owned native seams and loads no extension. Its immutable diagnostic `WKUserScript` is compiled only in the release-forbidden probe graph and runs in a dedicated non-extension world. |
| Native construction | `host/construction.rs` stages construction under a native resource lease, commit identity, callback permit, and cleanup-debt path. | The standalone M1 spike can test input without claiming product context ownership. Milestone 2 must reuse these lifecycle obligations rather than a parallel unbounded owner. |
| Navigation/presentation | Wry emits opaque navigation identity and guards presentation/input at commit; Zephium rejoins an `EventPermit` and `NavigationEpoch`. | Every later agent result must add context/owner/profile/frame/cancellation joins; URL or mutable current page state is not identity. |
| Stage/view presentation | `host/stages.rs` owns reveal/hide/reparent ordering and resumes Windows views when made visible. | Hidden input must not bypass presentation ownership or foreground another surface. |
| Suspension | `host/discard.rs` bounds Windows native suspends at eight; macOS relies on hidden/unmapped WebKit process policy. Discard safety joins event and navigation generations and cross-checks native activity. | Agent suspension cannot be inferred from one cross-platform API name. Owned-context lifecycle needs platform-specific proof. |
| Shutdown | `host/lifecycle.rs`, content-policy cancellation, platform cleanup debt, and shell shutdown enforce bounded ordered cleanup. | A probe pass requires drained work plus zero retained probe views; product contexts later join the same shutdown barrier. |
| Blocker | Shell policy produces generation-bound rules; engine content-rule compilation/application owns native registration and cancellation. | Agent contexts still require the normal native blocker pipeline. The M1 loopback spike does not bypass or weaken blocker policy. |

## Resource ceilings

`host/resources.rs` has one hard ceiling of 48 native webview-like resources.
Its disjoint class limits are 32 tab views, one warm spare, eight teardown-debt
slots, three extension background runtimes, one extension popup, one
reconciliation controller, and two transient-construction slots. The sum is a
compile-time assertion. There is currently no agent-context class or spare
capacity. Adding one is an explicit Milestone 2 resource-policy change, not an
incidental allocation.

The shell may persist up to 1,024 session `Item`s, but that is not permission to
construct 1,024 native views. Windows additionally caps native profile process
groups at eight and concurrent suspend requests at eight.

## Pinned native capability findings

- Wry is the reviewed 0.55.1 fork at
  `fe9e7fb73bb6ad2637cb0b4b1685676c86970aeb`; Tauri and
  tauri-runtime-wry are 2.11.3 at
  `6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a`.
- With no IPC handler, the fork installs no raw page-to-native message bridge.
  A handler can be registered to one exact `WKContentWorld`; Apple documents
  that the resulting JavaScript function exists only in that world. The M1
  probe uses one such handler for bounded one-way evidence and rechecks the
  world, name, webview, main frame, loopback URL, row, case, backend, protocol,
  phase, and payload size before attribution. Page world receives no handler.
- Wry exposes ordinary initialization/evaluation APIs, not a product agent
  content-world contract. Current WebKit source routes both public
  `evaluateJavaScript` and `callAsyncJavaScript` through a forced user gesture.
  The first authorized hidden run confirmed that even a fixture reset made
  `navigator.userActivation` sticky before input. Private no-user-gesture SPI
  is not an acceptable product or probe mechanism. The adapter now installs an
  immutable `WKUserScript` in a dedicated client world before navigation.
- Existing macOS extension diagnostics already route `NSEvent` mouse and key
  events to a WKWebView and observe trusted page events. Their windows are made
  key or receive application-queued events, so they do not establish safe
  invisible/background automation.
- macOS accessibility press is a public action-dispatch API, but its Boolean
  result is not effect verification and system permission cannot be assumed.
- Zephium's pinned Windows integration owns an ordinary child-HWND controller.
  `ICoreWebView2CompositionController::SendMouseInput` applies to composition
  hosting and is not evidence for the existing controller.
- The existing `windows-cdp-spike` is diagnostics-only and contains no
  `Input.dispatch*` route. It remains excluded from product/model contracts.

The complete machine-readable classifications and primary-source links are in
`capabilities-v1.json`.

## M0 implemented bounds

`zephium-agentic` now provides:

- an 8 KiB input / 64 KiB output versioned JSONL contract;
- a closed command vocabulary with no URL, selector, JavaScript, HTML, path,
  profile, native handle, provider, or free-form input;
- at most one admitted native-input run, 14 unique fixture cases, eight unique
  backends, 128 observations, and 64 events per observation;
- non-reusable generation-bound cancellation permits;
- closed redacted failures and a result schema containing only enums,
  booleans, bounded counters, durations, and validated runtime labels;
- a fixed loopback-only fixture server with three routes, a 4 KiB request cap,
  512-request lifetime, one worker, one-second I/O deadlines, no request logs,
  no external fetches, and no native bridge;
- compile-time optimized-build refusal for fixture/probe code plus a Cargo
  resolved-graph gate proving ordinary `zephium-desktop` cannot activate
  either diagnostic feature; probe-only modules remain feature-gated while
  production agentic contracts may enter the shipping graph.

## Open evidence gates

No Browse resource baseline, native-backend matrix, backend-order decision, or
real-site result is fabricated. The authorized hidden fixed-DOM safety matrix
is recorded, but AppKit/accessibility, visible/background native input, and all
physical-Windows evidence remain pending. Those gaps block M0/M1 qualification
but do not block deterministic harness and adapter implementation.

## M1 macOS adapter status

The feature-gated macOS adapter is implemented in the commit containing this
evidence. It owns one main-thread child WKWebView, a fresh explicit ephemeral
profile identity and non-persistent data store, no extension controller, no
page-world bridge, and no arbitrary evaluation API. It installs one immutable
release-excluded runtime at document end in a dedicated `WKContentWorld` and
one one-way handler scoped to that world. Every matrix row loads a fresh exact
loopback URL bearing only a bounded row id and closed case/backend enums, so
WebKit's sticky and transient user-activation state cannot contaminate a later
row.

The isolated runtime autonomously executes fixed DOM recipes and emits only
`ready`, `result`, or closed `fault` envelopes. The adapter also implements
direct AppKit event routing and in-process AppKit accessibility hit-test/press.
The latter does not use cross-process `AXUIElement` authority or prompt for
system-wide Accessibility access. Focused system input and the human baseline
remain explicit `NeedsHuman` outcomes; no diagnostic silently moves the real
pointer or types into the foreground session.

Popup request and admitted-page evidence are separate. The native new-window
policy callback records a request and synchronously denies it; an unexpectedly
admitted page is a verification failure. Cancellation, navigation/runtime
message deadlines, controller transport, server shutdown, and native view
teardown are bounded. All post-construction failure paths execute the same
explicit teardown before returning a redacted typed failure.

CI executes only the hidden fixed-DOM matrix. It requires 14 deterministic
terminal rows, zero trusted effect events, zero transient activation, zero
focus theft, popup denial, drained work, and zero retained native views.
WebKit-generated trusted focus/blur events are counted separately rather than
misclassified as input trust. The authorized 2026-08-31 macOS 27.0.0 / WebKit
22625.1.29.11.25 host run passed with zero trusted effect events, four trusted
focus/blur events, zero activation, zero focus theft, and zero retained views.
AppKit and accessibility behavior remains `pending_device_capture`; this result
is not a backend-order decision or real-site claim.

Visible-focused matrices additionally require the local
`--allow-visible-focused` process flag. Without it, admission returns the closed
`focus_policy_violation` failure before constructing a view. The flag never
authorizes global pointer or keyboard synthesis.
