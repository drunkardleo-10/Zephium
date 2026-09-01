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
request ceiling, and a 128 KiB response ceiling. Returned fixture values have a
32 KiB ceiling and a deny-unknown-fields schema. There is never more than one
CDP command in flight; the next command is issued only after successful
completion because WebView2 permits CDP calls to be processed out of dispatch
order.

| Route | Implemented behavior | Qualification state |
| --- | --- | --- |
| Fixed DOM recipe | One compiled fixture recipe evaluated with `userGesture: false` | Cross-compiled; hidden physical result pending |
| Ordinary child HWND | Validated CSS geometry is scaled to the owned child client area, then a maximum sixteen `SendMessageTimeoutW` mouse/key steps are issued under a 250 ms per-message ceiling | Cross-compiled; hidden/background physical result pending |
| Composition controller | No cast or call is made | `UnsupportedByIntegration`; pinned Wry creates an ordinary controller |
| CDP input | One fixed `Input.dispatchMouseEvent` or `Input.dispatchKeyEvent` completes before the next | Diagnostics only; physical result pending |
| Focused/human | No global input is generated | Human baseline remains explicit and visible |

The HWND route never calls `SendInput`, moves the system pointer, injects a
global keyboard event, or targets another process. Hidden and
visible-background presentation never call `SetForegroundWindow` or
`SetFocus`. The visible-focused runner mode requires the separate literal
`--allow-visible-focused` process argument.

## Bounds, cancellation, and teardown

- A matrix contains at most 112 rows and one row at a time. Input plans contain
  at most sixteen closed steps. Coordinates, DPR, client bounds, fixture
  events, JSON, navigation state, and durations are bounded.
- The run has a 90-second absolute deadline, each navigation a ten-second
  ceiling, each case a five-second ceiling, and native waits pump at most five
  milliseconds before rechecking cancellation.
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
refusals in the engine and contract crates. The production functional core may
now be reached through the durable Store adapter.

Physical Windows qualification must run the exact closed commands in
`eval/agentic-browsing/README.md` on an authorized named device. No code or
documentation here treats cross-compilation as behavioral evidence.

## Blocking evidence

1. hidden fixed-DOM, HWND, and CDP results on named Windows hardware/runtime;
2. visible-background no-focus-theft results on that device;
3. an explicitly authorized visible-focused human baseline if still needed;
4. one authorized difficult real-site slice;
5. Windows Browse startup, idle, concurrent-use, and input-latency baselines.
