# M1 Windows native-input probe

Status: release-excluded adapter cross-compiled; physical Windows behavior
pending.

This evidence describes code and compile-time facts only. It does not authorize
a product backend, claim event trust or site compatibility, or complete
Milestone 1 on Windows.

## Construction and authority

- A run owns one top-level probe HWND and one ordinary Wry child-HWND WebView2
  controller. It creates a unique temporary user-data directory, requests
  InPrivate mode, and verifies the runtime profile reports InPrivate before
  navigation.
- Construction and every matrix row re-attest the exact host, Wry container,
  WebView2 controller-parent, first document child, visibility state, and
  DPI-rounded 800-by-700 logical controller/container bounds. A navigation or
  runtime mutation cannot silently substitute a different native target.
- Browser extensions are explicitly disabled at environment construction. The
  unique UDF starts empty and no extension startup gate, package path, page IPC,
  host object, custom protocol, model input, selector, or caller JavaScript is
  configured. This is M1 isolation, not the M2 selected-profile/subprofile
  inventory proof.
- Top-level navigation is restricted to the exact ephemeral loopback origin and
  fixed routes. Every row uses a fresh correlation-bearing URL and must receive
  one Started, Committed, and Finished sequence with the same native Wry
  `NavigationId`; redirects, overlap, URL drift, and phase duplication fail the
  run.
- Popups, downloads, permissions, page close, host objects, web messages,
  autofill/password UI, context menus, script dialogs, external URI schemes,
  HTTP authentication, client certificates, and frame navigation outside the
  ordinary security policy remain denied.

## Observation and candidate routes

Fixture observation is diagnostic-only. The adapter invokes only compiled
`FixedProbeScript` expressions through `Runtime.evaluate`, always with
`userGesture: false`, `returnByValue: true`, no promise wait, a fixed 8 KiB
request ceiling, and a 128 KiB UTF-16/UTF-8 response ceiling. The adapter owns
the raw completion interface and performs that bounded scan before allocating;
webview2-com's convenience completion is intentionally not used because it
would construct an unbounded `String` first. Returned fixture values have a 32
KiB ceiling and a deny-unknown-fields schema. There is never more than one CDP
command in flight; the next command is issued only after successful completion
because WebView2 permits CDP calls to be processed out of dispatch order.
The dispatch function accepts only a closed three-variant method enum rather
than a string.

| Route | Implemented behavior | Qualification state |
| --- | --- | --- |
| Fixed DOM recipe | One compiled fixture recipe evaluated with `userGesture: false` | Cross-compiled; hidden physical result pending |
| Ordinary child HWND | Pinned Wry's public HWND is its `WRY_WEBVIEW` container, whose procedure forwards focus to the first direct child as the WebView document. The adapter scales fixed geometry to that direct child's client area, records the direct child's owning thread/process and input-locale handle, rejects a zero/calling-thread owner or invalid locale, and revalidates container/first-child/parent, owner identity, and locale before and after every message. It then issues at most sixteen `SendMessageTimeoutW` mouse/key steps. Each timeout is clamped to the lesser of the remaining case deadline and 250 ms, and `SMTO_ERRORONEXIT` makes receiver destruction fail the dispatch. Keyboard and character messages use `MapVirtualKeyExW(MAPVK_VK_TO_VSC_EX)` with the document thread's exact input locale, reject missing/unrecognized mappings, and preserve scan-code, extended-key, and up-transition fields. | Cross-compiled; hidden/background physical result pending |
| Composition controller | No cast or call is made | `UnsupportedByIntegration`; pinned Wry creates an ordinary controller |
| CDP input | One fixed `Input.dispatchMouseEvent` or `Input.dispatchKeyEvent` completes before the next | Diagnostics only; physical result pending |
| Focused/human | No global input is generated | Human baseline remains explicit and visible |

The HWND route never calls `SendInput`, moves the system pointer, injects a
global keyboard event, or targets an HWND outside its owned Wry subtree.
Hidden and visible-background presentation never call `SetForegroundWindow`
or `SetFocus`. Before the first fixture row they also require that the probe
host is neither foreground nor active and that this thread's keyboard focus is
outside the owned WebView subtree; otherwise the run returns a typed focus-
policy failure instead of treating the stolen state as its baseline. Every
row samples foreground window, active window, and thread keyboard focus before,
during, and after dispatch. Fixed native plans additionally sample immediately
before and after every HWND step and after every completed CDP step, accumulating
sticky focus/key-window facts across the plan. This detects transfers visible at
those boundaries; it does not claim system-wide event tracing or guarantee
detection of a focus transition and reversal entirely inside one synchronous
window procedure. Fixture focus/blur events remain independent DOM evidence. The
visible-focused runner mode requires the separate literal
`--allow-visible-focused` process argument.

Targeting Wry's container itself was rejected during pinned-code review:
`SendMessageTimeoutW` invokes the named HWND's procedure and does not perform
hit-testing or redispatch to a descendant. Wry's container handles only
`WM_SETFOCUS` specially. This correction landed before physical evidence, so
no result is attributed to the invalid container route.

Microsoft also documents that `SendMessageTimeoutW` calls the window procedure
directly and ignores `uTimeout` when the receiving window belongs to the
caller's queue. The candidate therefore refuses a document HWND owned by the
calling STA thread, records and revalidates the nonzero owner thread/process,
and the source gate forbids `AttachThreadInput`. This is a fail-closed runtime
precondition, not physical proof that a particular WebView2 build exposes the
expected cross-thread document child; the named-device runs must establish
that fact before the route can qualify. See Microsoft's
[`SendMessageTimeoutW`](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-sendmessagetimeoutw)
and
[`GetWindowThreadProcessId`](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid)
contracts.

The document thread's input locale can change dynamically. The probe binds the
fixed key sequence to the locale returned by
[`GetKeyboardLayout`](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getkeyboardlayout),
rechecks it at every target-identity boundary, and uses that exact handle with
[`MapVirtualKeyExW`](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-mapvirtualkeyexw).
A layout change aborts the row rather than allowing key-down, character, and
key-up messages to acquire inconsistent scan metadata.

## Bounds, cancellation, and teardown

- A matrix contains at most 112 rows and one row at a time. Input plans contain
  at most sixteen closed steps. Coordinates, DPR, client bounds, fixture
  events, JSON, navigation state, and durations are bounded.
- The run has a 90-second absolute deadline, each navigation a ten-second
  ceiling, each case a five-second ceiling, and native waits pump at most five
  milliseconds before rechecking cancellation. Every HWND step polls control
  and checks cancellation/deadline before dispatch and immediately after it;
  the synchronous message timeout cannot exceed the remaining case deadline.
- Explicit Wry close debt is retried for 500 ms and the bounded orphan-debt
  registry is drained. A sticky cleanup-overflow marker fails teardown.
- The adapter captures the exact browser PID and a non-reusable process HANDLE,
  installs the matching Environment5 process-exit observer, and requires both
  the exact callback and signalled HANDLE before deleting the temporary UDF. An
  uncertain process or controller retains the UDF and reports typed teardown
  failure rather than racing deletion.
- Evidence contains only closed enums, Booleans, counters, durations, and
  validated runtime labels. Paths, handles, PIDs, URLs, profile data, page
  content, native errors, and CDP responses are not emitted.
- Each physical mode writes one bounded versioned JSON response to its selected
  machine-evidence sink before qualification. A failed behavioral gate
  therefore retains redacted case evidence instead of collapsing to a generic
  process error; pass/fail diagnostics remain on stderr and local JSONL paths
  are gitignored.
- The required device workflow uses the runner's closed
  `--evidence-directory eval/agentic-browsing/local-results` option instead of
  shell redirection. Only the four required non-focused modes map to fixed
  filenames. Each destination must be absent before native work; validated
  UTF-8 protocol bytes are synced through an in-directory temporary file and
  atomically published without clobbering prior evidence. The runner rejects
  a Windows directory carrying `FILE_ATTRIBUTE_REPARSE_POINT`, including a
  junction or mount point rather than only a symbolic link.
- The offline review binary decodes those records through the same closed
  protocol and qualification core used by the runner. It accepts only the four
  fixed ignored filenames, direct regular files, exact response/run identity,
  exact matrix order and capability inventory, and one identical runtime
  fingerprint. It emits one content-free aggregate and never echoes raw records
  or paths. Its exact `--write-summary` option creates the fixed UTF-8 summary
  file and refuses to replace an existing result. Directory, input-record, and
  output preflights reject every Windows reparse point rather than relying on
  `FileType::is_symlink`, which is insufficient for NTFS junctions.

## Compile evidence and physical run path

The following host-independent gate succeeds for the pinned MSVC target:

```sh
cargo check --locked --target x86_64-pc-windows-msvc \
  -p zephium-engine --features native-agentic-input-probe \
  --bin windows-agentic-input-probe
```

The boundary checker requires both platform binaries and both platform modules
to remain behind `native-agentic-input-probe`, requires the probe-only
dependency set exactly, proves the ordinary desktop release graph cannot
activate either diagnostic feature, requires every diagnostic-only agentic
module to remain feature-gated, and retains independent optimized-build
refusals in the engine and contract crates. It also locks the Windows runner's
closed process gates, JSONL evidence path, owned-document HWND resolution,
bounds/visibility/controller ownership attestation, during-dispatch active and
thread-focus sampling, per-step deadline/cancellation checks, document HWND
thread/process/input-locale identity and non-calling-thread precondition,
target-layout `MapVirtualKeyExW` mapping for key and character messages,
receiver-exit failure, Windows reparse-point rejection for physical evidence,
strict bounded raw CDP completion, fixed method
allowlist, and absence of input-queue attachment, global `SendInput`, cursor
movement, page IPC, host objects, or generic script calls.
The production functional core may now be reached through the durable Store
adapter.

Physical Windows qualification must run the exact closed commands in
`eval/agentic-browsing/README.md` on an authorized named device. No code or
documentation here treats cross-compilation as behavioral evidence.

## Blocking evidence

1. hidden fixed-DOM, HWND, and CDP results on named Windows hardware/runtime;
2. visible-background no-focus-theft results on that device;
3. an explicitly authorized visible-focused human baseline if still needed;
4. one authorized difficult real-site slice;
5. Windows Browse startup, idle, concurrent-use, and input-latency baselines.
