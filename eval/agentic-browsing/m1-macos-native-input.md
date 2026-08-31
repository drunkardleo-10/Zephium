# M1 macOS native-input probe

Status: adapter and deterministic safety gate implemented; named-device
qualification pending.

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
- The configuration has no `WKWebExtensionController`. Wry receives no IPC
  handler, initialization script, userscript, custom protocol, file path, raw
  HTML, selector, or caller-supplied JavaScript.
- The only network surface is an IPv4-loopback listener with three fixed
  routes, one worker, 4 KiB request headers, one-second connection deadlines,
  and a 512-request lifetime. The worker blocks in `accept` and is woken by one
  loopback connection at shutdown rather than polling while idle. CSP denies
  external fetches.
- The new-window callback records a Boolean request fact and returns synchronous
  denial. The adapter never constructs a popup view.

## Fixed runtime seam

The adapter's evaluation type has five closed variants: fixture readiness,
fixture reset, fixture read, case geometry, and one fixed DOM recipe per case.
The trusted fixture's readiness/reset/read methods execute in page world. DOM
geometry and recipes execute in WebKit's default client content world. There is
no public string-evaluation function and no route from JSONL fields to script
source.

Every case/backend row gets a newly reloaded top-level document. After the
initial load, the adapter binds each wait to the exact native navigation object
returned by `reload`, then requires that identity's finished event and fixture
readiness before running it. This prevents a late or same-document callback,
`navigator.userActivation.hasBeenActive`, transient activation, focus, form
values, timers, or popup state from one row becoming evidence for another.

## Candidate routes

| Route | Implemented behavior | Qualification state |
| --- | --- | --- |
| Fixed DOM recipe | Fixed per-case recipe in the default client content world | Hidden CI gate only; emits untrusted events and must not activate/focus the process |
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
- The run has one 60-second absolute deadline; navigation and evaluation also
  have 10-second and five-second local ceilings. Settle is 90 ms and checks
  cancellation every run-loop slice.
- Cancellation is exact, non-reusable, and polled during navigation,
  evaluation, settle, and matrix iteration. Post-construction errors and
  cancellation converge on explicit view close, worker shutdown, and teardown
  observation before a result is returned.
- Evidence contains closed enums, Booleans, counters, durations, and validated
  runtime labels only. URLs, profile identifiers/paths, page text, HTML,
  screenshots, native errors, provider data, and clipboard contents are absent.
- Protocol/evidence version 2 separates a native popup request from an admitted
  popup page; version 1 evidence cannot be decoded as version 2 implicitly.

## Deterministic gate

The `--ci-hidden-fixed-dom` executable mode runs all 14 cases against one
backend and requires:

- a correct typed terminal result for every case;
- no trusted event and no transient activation;
- no probe/Browse focus theft;
- no admitted popup;
- a drained fixture worker and no retained native view.

The JSONL controller framing, capacity refusal, close-on-exec descriptor,
release graph, optimized-build refusal, target/capability joins, and fixture
server fragmentation behavior also have deterministic tests.

## Blocking evidence

The following remain intentionally absent rather than inferred:

1. named macOS hardware/OS/WebKit results for AppKit and accessibility across
   focused, visible-background, and hidden states;
2. an explicitly authorized visible focused-OS/human baseline;
3. the physical Windows HWND/composition/CDP feasibility matrix;
4. one authorized difficult real-site slice per platform;
5. Browse startup, idle, concurrent-use, and input-latency baselines.

No production backend order should be selected until those results exist.
