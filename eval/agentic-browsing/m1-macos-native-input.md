# M1 macOS native-input probe

Status: adapter and authorized hidden fixed-DOM safety gate qualified;
native-backend named-device qualification pending.

This evidence describes the release-excluded native-input risk spike. It does
not authorize a product backend, claim real-site compatibility, or complete
Milestone 1 on Windows.

## Construction and authority

- The executable owns process main and admits one run at a time.
- Visible-focused matrices are rejected at admission unless the local operator
  started the process with `--allow-visible-focused`. That flag permits only
  the probe window's explicit presentation; it does not permit global OS input.
- A fresh `ProfileId` names the run's ephemeral profile. WKWebView construction
  uses a non-persistent `WKWebsiteDataStore`, verifies persistence and
  identifier postconditions before navigation, and verifies the constructed
  page configuration again.
- The configuration has no `WKWebExtensionController`. Wry receives no generic
  IPC handler, custom protocol, file path, raw HTML, selector, or
  caller-supplied JavaScript. Before navigation, the adapter installs one
  immutable release-excluded `WKUserScript` and one handler into the same
  dedicated `WKContentWorld`; page world receives neither program state nor a
  handler function.
- The only network surface is an IPv4-loopback listener with three fixed
  routes, one worker, 4 KiB request headers, one-second connection deadlines,
  and a 512-request lifetime. The worker blocks in `accept` and is woken by one
  loopback connection at shutdown rather than polling while idle. CSP denies
  external fetches.
- The new-window callback records a Boolean request fact and returns synchronous
  denial. The adapter never constructs a popup view.

## Fixed runtime seam

The first authorized hidden run invalidated the original evaluation mechanism:
the fixture reported `navigator.userActivation.isActive` and
`hasBeenActive` before Button input. Current WebKit source confirms that both
public `evaluateJavaScript` and public `callAsyncJavaScript` pass
`forceUserGesture:YES`; only private SPI exposes a no-user-gesture variant.
The adapter therefore uses neither public evaluation nor private SPI.

The replacement is an immutable program installed at document end in a custom
client world. It accepts no native command, selector, expression, or reply. It
derives one row from an exact loopback URL whose values are emitted only from a
bounded integer and closed Rust enums, emits a `ready` envelope with validated
geometry, autonomously executes the compiled fixed-DOM recipe when selected,
and emits one `result` or closed `fault` envelope. The native handler is
registered only in that same content world and accepts a 32 KiB maximum string.
It rechecks handler name, world identity, webview identity, main-frame identity,
exact loopback URL without fragment, runtime protocol, row, case, backend,
phase ordering, geometry bounds, event bounds, and evidence schema. It provides
no reply and no native authority.

Every case/backend row gets a fresh top-level document at an exact unique URL.
The adapter arms the expected identity before navigation, then requires both a
new main-page finished generation and the correlated isolated-world `ready`
message. This prevents a late or same-document callback,
`navigator.userActivation.hasBeenActive`, transient activation, focus, form
values, timers, or popup state from one row becoming evidence for another.

## Candidate routes

| Route | Implemented behavior | Qualification state |
| --- | --- | --- |
| Fixed DOM recipe | Fixed per-case recipe in the dedicated immutable client runtime | Authorized hidden gate passed; effect events remain untrusted and the route does not activate/focus the process |
| AppKit event | Geometry-bound `NSEvent` mouse/key delivery to the owned window/WKWebView | Device run pending for each presentation state |
| AppKit accessibility | In-process accessibility hit-test followed by `accessibilityPerformPress` on supported public controls | Device run pending; dispatch Boolean is never treated as effect success |
| Focused OS input | No global event is posted | `NeedsHuman` in visible focus; otherwise `BlockedByPolicy` |
| Human baseline | No synthesized action | Always `NeedsHuman` |
| Windows routes | Inventory is present and explicitly unsupported by this adapter | Windows adapter and physical run pending |

In-process AppKit accessibility is deliberately narrower than cross-process
`AXUIElement` automation. It does not prompt for system-wide Accessibility
access and cannot target another application. If its measured coverage is too
small, that is an unsupported result rather than authority to broaden it
silently.

## Bounds, cancellation, and privacy

- Input and output are bounded to 8 KiB and 64 KiB JSONL records.
- One matrix contains at most 14 unique cases, eight unique backends, and 112
  rows. Each row records at most 64 allowlisted events.
- Controller ingress has 16 slots. Its duplicate stdin descriptor is
  close-on-exec and therefore does not leak into WebKit helpers. The reader
  blocks on stdin plus a close-on-exec stop pipe; it has no idle polling timer,
  and closing the pipe wakes shutdown without a signal-prone write.
- The run has one 60-second absolute deadline; each navigation has a 10-second
  ceiling and each correlated runtime result has a two-second ceiling. The
  fixed runtime samples settled activation after 50 ms and emits final evidence
  after 200 ms. Native waits check cancellation every five-millisecond run-loop
  slice.
- Cancellation is exact, non-reusable, and polled during navigation,
  runtime-message wait, teardown, and matrix iteration. Post-construction errors
  and cancellation converge on explicit view close, worker shutdown, and
  teardown observation before a result is returned.
- Evidence contains closed enums, Booleans, counters, durations, and validated
  runtime labels only. URLs, profile identifiers/paths, page text, HTML,
  screenshots, native errors, provider data, and clipboard contents are absent.
- Protocol/evidence version 2 separates a native popup request from an admitted
  popup page; version 1 evidence cannot be decoded as version 2 implicitly.

## Deterministic gate

The `--ci-hidden-fixed-dom` executable mode runs all 14 cases against one
backend and requires:

- a correct typed terminal result for every case;
- no trusted effect event and no transient activation;
- no probe/Browse focus theft;
- no admitted popup;
- a drained fixture worker and no retained native view.

Trusted focus and blur events generated by WebKit while a fixed recipe focuses
an editable control are counted separately: `isTrusted` alone is not accepted
as proof of native input. The authorized 2026-08-31 exact command completed 14
rows on macOS 27.0.0 / WebKit 22625.1.29.11.25 with zero trusted effect
events, four trusted focus/blur events, zero activation, zero focus theft, and
zero retained views. It used a hidden window, fresh ephemeral data store,
loopback-only fixtures, no system-wide input, no Accessibility prompt, no
accounts, and no external site.

The JSONL controller framing, capacity refusal, close-on-exec descriptor,
release graph, optimized-build refusal, target/capability joins, closed fixture
URL parser, isolated envelope parser, and fixture server fragmentation behavior
also have deterministic tests.

## Blocking evidence

The following remain intentionally absent rather than inferred:

1. named macOS hardware/OS/WebKit results for AppKit and accessibility across
   focused, visible-background, and hidden states;
2. an explicitly authorized visible focused-OS/human baseline;
3. the physical Windows HWND/composition/CDP feasibility matrix;
4. one authorized difficult real-site slice per platform;
5. Browse startup, idle, concurrent-use, and input-latency baselines.

No production backend order should be selected until those results exist.
