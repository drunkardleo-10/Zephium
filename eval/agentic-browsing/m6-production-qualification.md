# Milestone 6 production qualification

Status: in progress; Zephium is not yet production-qualified for agentic
browsing.

This is the reproducible engineering record for Milestone 6. It reports only
reviewed aggregate evidence and points to executable gates. It must not contain
raw probe JSONL, page content, screenshots, profiles, credentials, provider
responses, machine-local paths, or native traces.

The physical-Windows procedure now has a checked-in, source-gated two-phase
PowerShell orchestrator. It requires a clean exact checkout, create-new source
and debugger-binary hash stamps, an exact result inventory, the x86-64 MSVC
host toolchain, and an explicit authorized-device acknowledgement. It cannot
invoke focused input or the debugger-only mode. Final review rejects a
different checkout, a changed debugger executable, or any missing, reparse, or
unexpected record. This is workflow integrity only; no Windows device behavior
is claimed until the separately authorized run and human aggregate review
occur. Every Cargo command in both phases is offline; dependency acquisition
must finish before collection authority is exercised.

The first allowlisted public-site slice is now recorded in
`native-input-matrix-v1.json`: three of three hidden macOS/WebKit runs used
Terra to select and fill Wikipedia's live search control, then independently
verified the exact value and drained every native owner. A second three-of-
three slice exercised a complete continuation workflow: Terra filled the
search control, consumed the verified result, and selected the exact observed
`Deutsch` option. Every run completed in two model turns and both effects were
verified from fresh semantic snapshots. The page's autofocus changed only the
responder inside its hidden non-key window; application activation, visibility,
key/main-window state, and user focus remained strict failure conditions. A
third three-of-three slice forced the bounded semantic `locate` continuation
between the two actions and verified the same final selection. This does not
qualify native-select `input`/`change` event compatibility, difficult sites,
authentication, navigation, extensions, Browse concurrency, Windows, or the
shipping policy actor.

The first compact GPT-5.6 Luna slice is also recorded. Collapsed combobox
options remain in Rust's full semantic authority but are omitted from the model
projection; the control reports the bounded option count and explicitly marks
its option references as discoverable through `locate`. Geometry likewise
remains available to native freshness, hit-testing, occlusion, and action
revalidation while coordinates are absent from model input. A retained
public-page trace exposed one rejected Luna proposal that reused a visible
`Deutsch` link as a select option. Production binding correctly refused it as
an invalid selection target. The projection contract was then made explicit,
and qualification diagnostics now preserve the exact content-free binding,
checkpoint, or native-admission refusal class.

The corrected repeatability sweep passed three unconstrained and three
forced-locate workflows: six of six runs, twelve of twelve independently
verified native effects, and zero focus theft. The sweep consumed 78,533 input
and 1,823 output tokens, cost 4,810 micro-USD, spent 65,029 ms in the provider,
and completed in 71,282 ms wall-clock. Each unconstrained run independently
chose the bounded locate path. Initial semantic disclosure was 3,116 bytes per
run, compared with 10,663 semantic bytes across each earlier two-turn Terra
workflow before collapsed-option projection. These are exact evidence for this
one public page and task, not a multi-site or release qualification.

## Reusable production-path session: public Luna workflow

The next vertical uses `AgentBrowserSession` on the normal `provider-transport`
feature path, not the excluded synthetic `TerraProbeActionBridge`. See
[the session host contract](../../docs/agent-browser-session.md). One policy,
transport and cancellation owner handles up to eight model turns and eight
actions, including semantic locate and independently verified continuations.
At this earlier checkpoint the shipping text-only controller was unchanged;
Work actor integration and durable audit closure were still open. The following
Work actor section records the next vertical separately. Other provider adapters
and complete recovery remain open. This is not production release qualification.

The excluded host opens only public Wikipedia in a hidden ephemeral,
extension-free owned view. The task allows initial query preparation and language
selection in either order, then requires a refined query after both coexist in
verified state. The model chooses tools and ordering; the native loop is bounded
but has no fixed two-action callbacks. A trusted host predicate, not model text
or an effect count, terminates the task. No search is submitted, link clicked,
navigation performed after initial loading, or external record written.

Command, repeated independently three times on the qualification Mac:

```sh
cargo run --locked -p zephium-terra-macos-probe --features live-probe -- --live-public-luna-workflow-inspectable
```

The reviewed final-policy sweep completed 3/3 workflows and 9/9 independently
verified effects. Each run chose Fill → Select → Fill, used one semantic locate
turn, satisfied the initial and final task predicates, stole no focus, and
drained the page/window/store owners. Every successful mode re-attests native
isolation and CPU-throttled scheduling before retirement.

| Run | Model turns | Input tokens | Output tokens | Provider ms | Wall ms | Cost micro-USD |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 4 | 13,173 | 338 | 17,211 | 18,384 | 801 |
| 2 | 4 | 13,218 | 380 | 12,662 | 14,688 | 856 |
| 3 | 4 | 13,147 | 327 | 13,522 | 14,975 | 785 |
| Total | 12 | 39,538 | 1,045 | 43,395 | 48,047 | 2,442 |

Per-turn reviewed numerical receipts follow. Provider time includes exact-token
counting and execution; wall time is cumulative from native workflow start to
the receipt callback. Serialized request bytes count the model request body,
not HTTP headers or the separate counting request. Semantic bytes are newly
disclosed payload bytes, not the entire retained conversation.

| Run.turn | Input | Output | Request bytes | Semantic bytes | Provider ms | Cumulative wall ms | Micro-USD |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1.1 | 3019 | 114 | 16584 | 3116 | 3386 | 3904 | 198 |
| 1.2 | 3231 | 55 | 18927 | 287 | 2402 | 6553 | 180 |
| 1.3 | 3346 | 80 | 20935 | 208 | 9081 | 15641 | 190 |
| 1.4 | 3577 | 89 | 23204 | 408 | 2342 | 18141 | 233 |
| 2.1 | 3019 | 133 | 16584 | 3116 | 2787 | 3301 | 221 |
| 2.2 | 3250 | 48 | 19055 | 287 | 2293 | 5713 | 177 |
| 2.3 | 3358 | 82 | 20999 | 208 | 4806 | 10525 | 191 |
| 2.4 | 3591 | 117 | 23292 | 408 | 2776 | 14431 | 267 |
| 3.1 | 3019 | 105 | 16584 | 3116 | 4827 | 5355 | 187 |
| 3.2 | 3222 | 55 | 18863 | 287 | 2822 | 8719 | 178 |
| 3.3 | 3337 | 81 | 20843 | 208 | 2422 | 11149 | 191 |
| 3.4 | 3569 | 86 | 23112 | 408 | 3451 | 14733 | 229 |

Total serialized request bytes were 238,982 and disclosed semantic bytes 12,057.
Provider retention was explicitly enabled only for this public qualifier, using
fixed content-free metadata. OpenAI Logs remain exact provider-boundary truth;
local content-free receipts remain truth for binding, policy, native execution,
settlement, verification, timing and teardown. Production/BYOK remain stateless.
No provider payload, credential, page content, screenshot or raw trace is stored
in this engineering record.

Earlier investigation runs are excluded from the sweep. With WebKit's default
inactive `Suspend` policy, native requests reached the isolated channel but a
later snapshot reply stalled after two verified effects. Longer waits and a
request-scoped wake/restore did not fix that baseline. `Throttle` construction
policy keeps Rust-Ready owned pages runnable without disabling background CPU
limits; the live policy is now natively attested and source-gated. Browse policy
is unchanged. No idle battery/CPU/RAM qualification, macOS native suspend/resume,
concurrent-user interaction, authenticated site, navigation, or difficult-site
coverage is claimed.

Validation for this slice: 507 functional-core library tests, 7 controller tests,
457 engine library tests with `agentic-browser`, 2 task-postcondition tests and
165 xtask tests pass. Strict all-feature/all-target Clippy covers the engine,
controller, qualifier and xtask. Controller, native-probe, model-catalog and
runtime source gates pass, including the semantic JavaScript smoke fixture.
The hidden native fixed-DOM regression passes 10 snapshots across 4 epochs with
verified click/fill, stale-anchor refusal/recovery, no focus theft and no retained
views. The normal `provider-transport` controller compiles optimized; enabling
the qualification harness in an optimized build is correctly compile-refused.

## Production-path Work actor: public Luna workflow

The 2026-09-05 pass uses `AgentWorkController` on the normal
`provider-transport` path, the existing runtime worker/mailbox, the real
`EngineHost::take_agent_browser_port`, and the durable `SqliteStore` audit port.
The excluded wrapper only installs an explicit policy for its isolated public
profile, pumps the real native dispatcher, checks focus and collects content-free
metrics. It does not implement a fake browser port or substitute qualification
policy/native owners. See [the actor contract](../../docs/agent-work-execution.md).

```sh
cargo run --locked -p zephium-terra-macos-probe --features live-probe -- --live-public-luna-work-actor-inspectable
```

The same public preparation/refinement task is terminated by the trusted
predicate, not three effects or model text. Three integration runs completed with
four provider turns and three verified effects each, no focus theft, and clean
native/provider/policy/audit/runtime/store closure. The first run preceded final
schema pruning; the two final-schema runs advertise only supported snapshot
actions and immediate/mutation-quiet waits. This gives 2/2 final-schema workflows
and 6/6 effects (3/3 actor integrations and 9/9 effects across the pass), not a
controlled performance comparison.

| Configuration | Input tokens | Output tokens | Request bytes | Semantic bytes | Provider-turn ms | Total wall ms | Charged micro-USD |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Initial actor integration | 13,173 | 347 | 79,594 | 4,019 | 10,687 | 12,717 | 814 |
| Bounded native-adapter schemas, run 1 | 11,217 | 385 | 61,460 | 4,209 | 10,650 | 12,201 | 1,407 |
| Bounded native-adapter schemas, run 2 | 11,216 | 389 | 61,692 | 4,113 | 9,467 | 10,887 | 834 |
| Final-schema total | 22,433 | 774 | 123,152 | 8,322 | 20,117 | 23,088 | 2,241 |

Final-schema per-turn receipts:

| Run.turn | Input | Output | Request bytes | Semantic bytes | Provider-turn ms | Cumulative wall ms | Charged micro-USD |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1.1 | 2512 | 103 | 12000 | 3116 | 3351 | 3933 | 752 |
| 1.2 | 2713 | 60 | 14259 | 287 | 2363 | 6915 | 174 |
| 1.3 | 2878 | 85 | 16454 | 398 | 2446 | 9363 | 199 |
| 1.4 | 3114 | 137 | 18747 | 408 | 2490 | 11986 | 282 |
| 2.1 | 2512 | 109 | 12000 | 3116 | 2517 | 3158 | 182 |
| 2.2 | 2719 | 71 | 14323 | 287 | 2416 | 5786 | 188 |
| 2.3 | 2872 | 90 | 16528 | 302 | 2054 | 7843 | 202 |
| 2.4 | 3113 | 119 | 18841 | 408 | 2480 | 10538 | 262 |

These are policy-charged `PricedCeiling` receipts, not a provider invoice or a
claim of server-only inference time. Provider-turn time includes exact token
counting and streaming. Request bytes exclude HTTP headers and the separate
count request; semantic bytes count newly disclosed input. Explicit public
OpenAI Logs remain provider-boundary truth, while local typed receipts prove
projection, policy, native execution, settlement, audit and teardown.

Real integration exposed three previously unexercised seams: the runtime timer-
only worker lacked I/O, committed navigation did not yet prove observation
readiness, and adjacent snapshot verification must follow the core's settlement
wake rather than repeated snapshot polling. The fixes enable I/O only on the
existing active worker, refuse premature observation as NotReady, fence late
navigation callbacks after revocation, and consume the exact settlement schedule.
No longer wait, blind native retry, relaxed freshness or fabricated proof was used.

The default desktop still has no Work run-admission/persistence adapter. Explicit
failure reconciliation, approval resume, macOS native suspend/resume, more tools,
authenticated sites, concurrent Browse and battery qualification remain open.
Recovery is retained ownership, not success or a resumable user product yet.

Validation: 508 functional-core, 15 controller, 35 runtime, 458 engine
(`agentic-browser`), 245 store, 166 xtask and 2 qualifier-predicate tests pass.
Controller tests exercise eight variable provider turns through the real worker's
localhost I/O; count/stream refusal and takeover races; native admission,
observation, renderer, cancellation, deadline and lifecycle shutdown faults;
lost observation/cancellation/close/audit callbacks; close refusal; native-action
dispatch/terminal refusal, takeover and callback loss; exact policy NeedsHuman
and retained prepared batches; mailbox and product backpressure. These fixtures
do not claim native-device fault injection.
Strict all-feature/all-target Clippy covers controller/runtime/core/engine/store/
qualifier/xtask. Production optimized controller and engine checks pass. Runtime,
controller, model-catalog, native-adapter and broad probe/source gates pass.
The default desktop dependency graph contains no controller, agent runtime,
provider transport or reqwest; the runtime has no provider dependency.
CI explicitly lints/tests the enabled controller path despite its intentional
absence from the default desktop graph. Optimized probe-harness compilation is
correctly refused.

## Mechanically enforced evidence

`cargo xtask check-agentic-probe-boundary` is part of ordinary CI and now:

- decodes `browse-baseline-v1.json`, `native-input-matrix-v1.json`,
  `semantic-runtime-macos-v3.json`, and `capabilities-v1.json` with closed
  schemas and file-size ceilings;
- rejects unknown fields, machine-local paths, and high-confidence secret or
  raw-content fields;
- requires the named-device Browse baseline to remain empty while its status is
  pending;
- binds the reviewed macOS hidden fixed-DOM aggregate to its exact command,
  platform, backend, case count, trust/focus/activation results, and teardown
  result;
- source-gates the release-excluded macOS input qualifier itself: its ephemeral
  data store, extension-free fixed content world, page/frame/URL and payload
  joins, and cancellation/deadline checks immediately before navigation,
  presentation/focus changes, each AppKit event, accessibility hit-test, and
  accessibility press are mandatory. The gate also forbids global `CGEvent`,
  system-wide Accessibility trust/prompt, generic page-evaluation, and generic
  IPC authority, and mutation-tests representative removals;
- binds the reviewed macOS production semantic aggregate to its exact command,
  OS/WebKit build, closed viewport, five snapshots across three world epochs,
  host-released mutation, stale-anchor refusal, recovery, bridge, redaction,
  focus, teardown, and explicit non-claims;
- prevents the cross-compiled Windows runner from being represented as
  device-qualified without a reviewed gate change;
- cross-checks the Wry, Tauri, and `tauri-runtime-wry` capability versions and
  revisions against their vendored manifests and provenance files; and
- continues to prove that diagnostic features and the dormant provider HTTPS
  transport are absent from the ordinary desktop release graph; and
- independently compile-refuses the fixed macOS semantic qualifier in
  optimized builds and proves it absent from that graph.
- enumerates all eight agentic-owned production platform modules that contain
  native/unsafe boundaries and requires each module to deny both undocumented
  unsafe blocks and unsafe operations outside explicit unsafe blocks. A
  mutation test removes or misplaces each lint header and proves the gate
  refuses the source; and
- requires the agentic functional-core crate, dormant provider transport, and
  all 16 dedicated production engine modules to deny direct standard-output,
  standard-error, and `dbg!` macros. Every non-diagnostic functional-core
  source and each dedicated engine/provider source is also scanned for direct
  Rust and native logging calls. The provider gate separately pins both
  retained credential owners and the transient header-construction buffer in
  zeroizing storage, forbids an extra library-owned header construction during
  admission, marks the sole dispatch-time request header sensitive, manually
  redacts credential/attempt `Debug`, and uses a closed reqwest failure
  classifier that consumes only the timeout bit. Pinned `http` 1.4.2 copies a
  byte slice into ordinary `Bytes` for `HeaderValue` and has no zeroizing drop,
  so the evidence does not claim zeroization for that unavoidable HTTP-stack
  or wire copy. The gate also requires sticky cancellation before authentication
  materialization and a second check after request construction but before the
  send future can be polled. Mutation tests prove each protection and ordering
  edge is required; and
- requires the agentic functional core, dormant provider transport, and the
  same complete inventory of 16 dedicated production engine modules to deny
  direct Clippy `unwrap`, `panic`, and `unreachable` findings outside test
  builds. The Windows semantic-protocol gate additionally requires the three
  parameter-free control commands to use one compile-time-bounded literal
  payload and rejects a fallible JSON-construction/`expect` regression. These
  checks cover explicit invariant-abort mechanisms in owned source; they do
  not prove freedom from allocation failure, bounds-check panics, dependency
  defects, or every possible Rust panic source; and
- pins the provider's 64 KiB response-header ceiling at both available layers:
  reqwest configures the pre-decode HTTP/2 list limit, then a zero-allocation
  checked pass accounts every decoded field's name, value, and 32-byte
  protocol overhead before status, retry-hint, content-type, or body handling.
  This second pass includes HTTP/1 fallback, for which pinned reqwest exposes no
  Hyper receive-buffer control. Successful responses also refuse a duplicate,
  non-canonical, or exact-call-over-budget `Content-Length` before streaming,
  while allowing omission for SSE. Exact-limit/over-limit tests and source-order
  mutations make the accepted response boundary explicit; and
- pins HTTP/2 receive pressure rather than inheriting mutable library defaults:
  both the initial stream and connection flow-control windows are 65,535 bytes,
  adaptive growth is disabled, and the maximum accepted frame is 16,384 bytes.
  Source mutations prove that each setting and exact value is required. These
  are protocol-level ingress bounds, not a TLS, socket, HTTP/1, or process-memory
  qualification; and
- distinguishes an observationally idle provider transport from shutdown
  quiescence, which requires both its sticky cancellation/admission seal and
  exactly zero active slots. The gate pins constructor closure, open-admission
  and pending-attempt refusal, permanent sealing across shared clones, a
  bounded redacted proof, and mutation coverage for every critical predicate.
  The proof is non-authorizing and owns no provider, policy, audit, native,
  task, timer, or content seam. The async drain path must seal before returning
  its sole admitted future, refuse a concurrent waiter, register before
  sampling, wake on exact last-slot release, and race one absolute deadline
  without a polling worker; and
- requires run cancellation and provider-transport shutdown to linearize with
  semantic disclosure rather than relying on adjacent atomic loads. The gate
  pins one zero-worker commit mutex per cancellation authority, both exact
  commit-gate acquisitions, the final cancellation check, policy commit and
  transport-slot commitment under those gates, shared-authority deadlock
  avoidance, and poison-to-sticky-cancellation behavior. Mutations that remove
  either gate, the final check, poison fail-stop, or release a gate before slot
  commitment are rejected; and
- pins the Windows owned-context suspension adapter to its feature/target gate,
  fixed atomic callback/timeout/cancellation owner, one late-reconciliation
  path, ten-second UI-thread deadline, `TrySuspend`/`IsSuspended`/`Resume`
  readbacks, ambiguous-state audit refusal, callback retirement at teardown,
  and content-policy replacement exclusion. Mutation tests remove the late
  state and teardown retirement and prove both changes fail; and
- forbids releasing an explicit profile lease from the lease value alone.
  Release now consumes the constructor-closed supervisor receipt emitted only
  after queued cancellation or terminal registry reaping, rejoins the exact
  context identity, and admits only the native-resource disposition compatible
  with the immutable context kind. Mutation tests remove the receipt, identity
  join, closed disposition, and constructor gate and prove each change fails;
  and
- makes the logical context shutdown seal retain every never-started row as a
  bounded cleanup obligation. New admission remains permanently closed, but
  exact supervisor cancellation is still allowed to remove each queued row and
  mint its cleanup receipt. The gate rejects seal-time removal, sealed-state
  rejection in cleanup, or validation that treats retained queued rows as an
  invariant failure; and
- gives the stable native browser port an atomic shutdown barrier. Its one
  mutex acquisition seals every mutation, semantic, action, and capture route
  before reserving the final audit; a full queue or rejected outer dispatcher
  cannot reopen admission. The shutdown audit has a distinct event type and
  exact correlation, while ordinary resource audits alone remain boundedly
  admissible after the seal for asynchronous drain checks. The gate rejects a
  reordered seal, a reopened mutation route, a seal-blocked audit, ordinary-
  audit substitution, or host settlement that bypasses the task-owned route.
- pins the zero-idle native shutdown coordinator to a consuming cohort of six
  exact logical owners: contexts, profile leases, cookie transfers, native
  action execution, post-action settlement, and screenshot capture. Every
  owner must already be permanently sealed and empty; refusal returns the
  complete cohort for continued cleanup. Screenshot capture now has a matching
  retain-and-drain seal. The coordinator distinguishes the first atomic-seal
  settlement from later ordinary audits, requires strictly increasing audit
  identities, caps all attempts at eight, checks all nine validated native
  resource counters for exact zero, and is the only constructor of the
  move-only terminal proof. Mutation tests remove each critical join, seal,
  distinct settlement, bound, zero-count check, and constructor closure and
  prove the release gate refuses the source; and
- pins the terminal native shutdown driver to an already-admitted consuming
  coordinator, keeping the lossless logical-owner refusal outside the driver.
  It also pins an existing stable port, a shell-minted first identity, one
  caller-owned event source, and one absolute deadline. It rechecks the
  deadline around dispatch,
  accepts only the exact event class for the pending coordinator stage, uses
  checked successor identities, and applies fixed 100 ms exponential retry
  waits capped at one second and eight total audits. Its manual diagnostics
  redact the event payload, and the source gate forbids worker, timer, channel,
  network, file, browser-engine, script-evaluation, or logging authority.
  Mutation tests prove the port calls, event separation, checked identity,
  deadline checks, redaction, and authority exclusions are required.
- pins the stable application lifecycle to one `Send`, move-only consuming
  shutdown operation under the caller's absolute deadline. Its exact closed
  outcome has no clean variant without the native zero proof and no retryable
  result after ownership is consumed. The lifecycle contract additionally
  requires cancellation, provider, durable-audit, policy, and logical-owner
  settlement; the native proof alone does not imply those facts.
- pins the application integration behind one non-default optional dependency
  that the ordinary desktop graph cannot enable. Separate agentic spawn and
  failure types preserve both move-only agent and extension lifecycle owners
  across each helper-worker refusal and disconnected actor handoff. The shared
  spawn function carries those owners directly through its diverging failure
  branches and one successful handoff, with no temporary `Option` or
  release-path `expect`; a function-local Clippy contract and source mutation
  gate reject `unwrap`/`expect`, `panic`, or `unreachable` regressions. The Shell
  consumes agent shutdown after retryable durability preflight and before
  extension, terminal Store, and engine teardown; `Clean` joins the proof,
  while unclean results and panics still run every later barrier. The same
  consuming cleanup is mandatory on unexpected actor exit.
- pins the durable audit Store boundary to two isolated, unsafe-free modules:
  the actor authority/settlement adapter and transactional SQLite adapter.
  Both compile-deny direct diagnostics and invariant-abort macros outside
  tests. The source gate mutation-tests the lazy eight-batch permit, checked
  fail-closed release, nonblocking shutdown join, bounded Store-mailbox send,
  exact pre-admission refusal proofs, uncertain-commit callback non-invocation
  and ledger replay retention, callback-panic containment, and the only two
  fixed content-free diagnostics. Caller-owned callback destructors are also
  panic-contained on every refusal or ambiguous discard path; this is an
  unwind-build boundary and not a claim of recovery after a release abort.
  This adds no task, connection, timer, allocation, or queue while agents are
  unused and does not by itself compose the full application runtime.

The same check pins the default functional core to an empty feature set and a
closed allocation/data dependency inventory. Its non-diagnostic source is
rejected if it acquires thread, process, file, socket, async-runtime, or HTTP
client authority. This is a structural zero-idle-authority proof: it does not
replace the pending named-device CPU, memory, wakeup, or energy measurement.
The gate also pins the receipt-derived accounting reducer to exact manifest
revision checks, replay-safe out-of-order identities, run/node budgets, and an
eight-schedule attribution ceiling, while forbidding it from acquiring a
telemetry or persistence port. A separate gate pins the audit-derived progress
reducer to the canonical event revision seal, strict event/time ordering,
bounded topology and active-operation storage, optional observed durations,
and the absence of runtime clocks, serialization, or telemetry ports. A third
gate pins the exact batch-terminal action reducer to fixed duration buckets, a
1 KiB snapshot ceiling, manifest/node authority, batch/effect/attempt replay
indexes, the manifest operation ceiling, and bounded preflight storage while
forbidding telemetry, persistence serialization, and unbounded maps. The
provider-input gate additionally pins the six closed semantic/screenshot stats
variants, scalar token-quality projections, exact request-byte bound, 64-byte
value ceiling, commit-only accessors, and propagation through the trusted HTTPS
attempt. It rejects content allocations and serialization fields in that
metrics value. A fourth reducer gate pins the at-most-192-byte commit-minted
receipt to a crate-private canonical manifest-revision join, then pins the
optional reducer to sorted replay rejection, canonical bounded plan-node
storage, run/node operation ceilings, transactional reservation, all six
closed input classes, and a 1 KiB snapshot ceiling. It forbids public receipt
construction plus telemetry, persistence serialization, runtime clocks/tasks,
unbounded maps, and browser/native ownership.

Run the gate and its focused tests with:

```sh
cargo test --locked -p xtask agentic_evidence
cargo test --locked -p xtask agentic_probe_boundary
cargo test --locked -p zephium-app --features agentic-browser
cargo xtask check-agentic-probe-boundary
cargo clippy --locked -p zephium-agentic --all-features --all-targets -- -D warnings
cargo clippy --locked -p zephium-agent-provider-transport --all-targets -- -D warnings
cargo clippy --locked -p xtask --all-targets -- -D warnings
cargo clippy --locked -p zephium-app --all-targets --features agentic-browser -- -D warnings
cargo clippy --locked -p zephium-engine --features agentic-browser --all-targets -- -D warnings
cargo clippy --locked -p zephium-engine --features agentic-browser --all-targets --target x86_64-pc-windows-msvc
```

The Windows cross-target command intentionally leaves unrelated target-CFG
dead-code warnings at their existing warning level. The module-level denies
remain errors, so that command still rejects undocumented unsafe code in the
agentic-owned Windows modules. Cross-compilation is not device behavior.

## Reviewed results

- The authorized macOS hidden fixed-DOM safety matrix completed 14 cases with
  zero trusted effect events, four separately classified trusted focus/blur
  events, zero activation rows, zero focus-theft rows, and zero retained native
  views. This qualifies only that deterministic safety route.
- The separately authorized production semantic adapter qualifier passed on
  macOS 27.0.0 / WebKit 22625.1.29.11.26. One hidden, extension-free,
  ephemeral 1280-by-800 logical view produced five checked snapshots across
  three isolated-world epochs. A private-ref fixed semantic click changed only
  the intended fixture's declared `Expanded` state from false to true, remained
  untrusted page input, granted zero transient/sticky user activation, admitted
  no popup, and passed the existing settlement and independent fresh-snapshot
  verification cores. Page-world bridge exposure and focus
  theft remained absent and all retained native owners drained. This qualifies
  the compatible fixed recipe and adapter path, not actual policy assessment,
  the full host/controller path, general trusted/native input, arbitrary sites,
  Windows, provider-token flow, or Browse/resource behavior.
- The allowlisted Wikipedia workflow completed three of three independent
  hidden macOS/WebKit runs with GPT-5.6 Terra. Each run used two model turns,
  applied one exact-value fill and one fixed native-select recipe, independently
  verified both effects, stole no application focus, and drained teardown. This
  qualifies that static public native-select slice only; it does not establish
  framework-event, navigation, authenticated, difficult-site, extension,
  concurrency, Windows, or shipping-policy compatibility.
- The same public workflow completed three of three additional runs through
  the explicit semantic-locate continuation. Each run used three provider
  turns; the middle turn produced a bounded text query, Zephium's fixed Rust
  matcher returned the exact observed option projection, and the final action
  again passed fresh-snapshot selection verification. This qualifies the
  controller/provider locate handoff for that exact public slice, not general
  fuzzy retrieval or arbitrary-site coverage.
- The Windows adapter is source-guarded and cross-compiles. Cross-compilation is
  not physical-device behavioral evidence. Its owned-context port now admits
  hidden suspend/resume through WebView2 only, with a ten-second UI-thread
  deadline, exact callback/timeout/cancellation ownership, retained late
  reconciliation, final `IsSuspended` readback, content-policy exclusion, and
  resource-audit refusal while physical state is ambiguous. macOS remains
  unsupported at this port. No suspend/resume device or resource claim is made.
- The physical Windows M1 reviewer now refuses a success label without the
  case-specific effect event on the exact intended target. HWND and CDP rows
  must carry `isTrusted` on that exact qualifying event, so an untrusted effect
  plus an unrelated trusted event cannot promote a candidate route. It refuses
  any retained effect for a non-dispatch route and cross-checks link, clipboard,
  and popup fields against the claimed terminal outcome. The release boundary
  mutation-tests those joins. The Windows CDP candidate additionally polls
  control and rejects cancellation/deadline immediately before every fixed
  native submission, rather than first discovering revoked authority after the
  command was queued. The qualifier now applies the same exact permit/deadline
  boundary before run allocation, Wry construction, loopback navigation, every
  visibility/focus transition, and immediately before each fixed input
  submission. Because Win32 `GetFocus`/`GetActiveWindow` expose only the
  caller's attached queue, the adapter also samples the revalidated owned
  document thread through read-only `GetGUIThreadInfo`; failure to obtain that
  projection rejects the evidence. The offline qualifier now also requires the
  three coarse focus-owner samples to match the selected presentation, and
  rejoins the serialized target DOM-focus bit to a retained focus event on the
  exact intended target; clearing the summary theft Booleans cannot hide a
  contradictory serialized focus sequence. Qualified rows additionally require
  exactly one Browser-kind process plus a joint nonzero bounded derived helper-
  process and aggregate Environment8-cohort resident-working-set sample on both
  sides of the action. The cross-compiled adapter rejects duplicate/invalid or
  changed PID/kind cohorts, partial query-limited handle or memory queries,
  close failure, overflow, and more than 64 total processes; it holds the
  handles through an exact post-sample Environment8 PID/kind/total/helper rejoin.
  WebView2 excludes crashpad from this API, so the sum is not a whole-process-
  family or resource-budget measurement. Its exact-pinned raw bindings remain
  optional behind the probe feature, and review schema v2 retains both maxima
  without emitting identity. After the
  final HWND control poll, the adapter samples both queues, rejoins document
  HWND/thread/process/layout, and only then derives the bounded window-message
  timeout. CDP uses the same final-poll/sample/submission order. The source gate
  rejects an added direct native dispatch site and mutation-tests representative
  admission, navigation, presentation, HWND, CDP, document-focus, and adjacency
  preflight removal. It
  also forbids alternate asynchronous/callback/broadcast
  message dispatch, synthetic input, window capture/activation, and composition-
  controller input paths around the sole audited HWND/CDP sites. This
  strengthens future evidence admission and cancellation; it is not physical
  Windows behavior evidence.
- The preferred physical-Windows orchestrator now runs the repository boundary
  gate, complete offline probe-harness tests, production agentic engine tests,
  and both native Clippy/link builds before creating its source stamp. It then
  rechecks the clean exact revision and empty create-new evidence directory
  before any native run. A failed or source-mutating preflight therefore cannot
  be mistaken for a partial evidence cohort. The static gate pins the command
  inventory and preflight→recheck→stamp→run order; no Windows behavior is
  inferred until the separately authorized physical records pass review.
  The wrapper and locked metadata lookup force Cargo offline throughout both
  phases; a missing local dependency fails before collection can create the
  source stamp.
  The existing Windows CI job parses the complete PowerShell source through
  the native language parser without executing a phase; the release gate pins
  that parse step beside the native Clippy/link jobs. This is syntax evidence,
  not physical behavior evidence.
- The manual debugger handoff no longer assumes Cargo's default target
  directory or trusts any compatible probe executable. The orchestrator
  resolves the active target directory from locked Cargo metadata, verifies
  the executable is a direct non-reparse file, and hashes it before collection.
  The final build must retain that digest before it is recorded in a create-new
  stamp. The review phase resolves and hashes the file again before it accepts
  the debugger record. The static gate pins both joins and
  preflight-hash→collection→final-build→stamp→handoff→review ordering; the
  script still cannot launch or authorize a debugger.
- The physical Windows semantic source gate now binds its content-free success
  fields to the closed snapshot verifiers, exact flood and renderer-loss
  refusals, native suspend/readback/resume result, semantic drain audit, and
  teardown result. It also requires stable nonzero bounded WebView2 Environment8
  process and aggregate resident-working-set observations before mode work and
  after the semantic drain. Both Windows qualifiers share one release-excluded
  sampler: exactly one Browser kind and at least one helper are required;
  query-limited non-inheritable handles remain owned through an exact
  PID/kind/total/helper cohort rejoin, and any partial, changing, overflowing, or
  close-failed observation is rejected. WebView2 omits crashpad, so this is not
  a whole-process-family or resource-budget measurement. Protocol and review
  schema v5 retain only the two maxima. Mutation tests replace each evidence
  class or resource producer with an unconditional value and prove release
  qualification fails.
  The sampler and its exact-pinned Win32 dependency are absent from ordinary
  engine builds. This is producer integrity and cross-compile coverage, not
  physical Windows behavior or a resource-budget claim.
- The macOS diagnostic input adapter now carries the same module-level unsafe
  lint contract as the Windows input adapter. Every current Objective-C unsafe
  operation has a local ownership, lifetime, thread, selector, or exception
  boundary rationale, and host native-feature Clippy passes with warnings
  denied. Its dispatch-control object polls the exact permit and absolute
  deadline adjacent to every native input effect and between multi-event
  sequences. This is compile/source evidence only: the already-reviewed hidden
  fixed-DOM run did not exercise AppKit or accessibility, and their named-device
  matrices remain pending.
- The functional agentic core forbids unsafe code. The agentic-owned production
  native graph currently contains exactly eight audited modules: the macOS and
  Windows owned-context, semantic-runtime, and semantic-screenshot adapters,
  plus the Windows cookie and UI-timeout adapters. Every unsafe block in those modules now
  carries a local lifetime, thread/apartment, pointer, ownership, or ABI proof,
  and module-level lints make both missing proofs and implicit unsafe operations
  compile failures. The audit also made macOS view hardening/attestation refuse
  off-main-thread calls and made the window-less Windows timer guard `!Send`, so
  its TLS callback and `KillTimer` obligation cannot move off the creating UI
  thread. Host Clippy and Windows MSVC cross-target Clippy passed this scoped
  audit. This does not qualify physical Windows behavior or claim that shared,
  extension-owned, third-party, or non-agentic platform code has completed its
  native audit.
- The scoped agentic-owned production graph now has a mechanically enforced
  no-direct-logging boundary: observability remains content-free through typed
  audit, metric, and shell values rather than page/provider output. The dormant
  provider transport retains credentials only in zeroizing buffers, builds no
  throwaway library-owned header during admission, marks the sole
  dispatch-time authentication header sensitive, redacts credential-bearing
  `Debug` implementations, and maps reqwest failures to closed classes without
  formatting the underlying error. A regression test against pinned reqwest
  constructs a request and proves neither its credential nor request-body
  sentinel appears in `Request` debug output. Zephium's retained and temporary
  credential buffers are zeroized; pinned `HeaderValue`, the TLS stack, and
  wire buffers are not claimed to be. Sticky cancellation now short-circuits
  before authentication materialization when observable at execution entry,
  while the existing second check still closes request construction before
  dispatch. This is a scoped source and test audit; it does not claim review of
  shared shell, extension-owned, third-party, or non-agentic logging surfaces.
- The provider transport now distinguishes an open but idle snapshot from a
  terminally quiescent one. After its sticky seal cancels retained attempts and
  permanently refuses admission, only an exact zero-active snapshot can mint a
  constructor-closed, move-only shutdown proof. This closes the transport-slot
  drain fact only: every attempt's policy/usage result still requires terminal
  settlement, and no concrete application composition or live provider result
  is claimed. Its cancellation-safe async wait seals before returning a future,
  bounds shared wait admission to one, wakes on the exact last slot, and fails
  closed at one absolute deadline; it creates no background worker or periodic
  polling timer.
- Provider semantic disclosure is now linearized against both its exact run
  cancellation and the shared shutdown root. Cancellation blocks only behind
  an already-running synchronous policy commit; once it becomes sticky, a
  later admission cannot commit page-derived input. Conversely, a commit that
  wins marks its transport slot committed before releasing either gate and is
  therefore conservatively settled as post-commit. Unit tests cover the
  concurrent gate, poison fail-stop, and shared-authority no-deadlock cases;
  the release boundary mutation-tests every critical ordering edge. This is
  deterministic concurrency evidence, not a live-provider qualification.
- A panic from the external normalized provider-batch consumer no longer
  unwinds past the transport and drops the exact active model authority in
  development/evaluation builds. The transport catches that boundary,
  permanently cancels and seals every clone, returns one non-retryable
  `Integration` failure with conservative post-dispatch accounting, and lets
  policy release the full reservation through its ordinary terminal path. A
  loopback fault test proves returned authority, shared quiescence, future
  admission refusal, and exact settlement. The release gate mutation-tests the
  catch, seal, failure class, and retry refusal. Optimized desktop remains
  process-terminal under `panic = "abort"`.
- Provider response headers now have a protocol-neutral decoded 64 KiB
  admission check in addition to reqwest's HTTP/2 pre-decode setting. Checked
  accounting runs before status and body interpretation and rejects one
  additional field beyond the exact ceiling. Pinned reqwest/Hyper source shows
  that HTTP/1 may allocate within Hyper's larger internal receive ceiling
  before this check; this change makes accepted processing bounded but does not
  claim a smaller pre-parser HTTP/1 allocation or live-provider behavior. A
  declared successful-response body now also fails before streaming if its
  `Content-Length` is duplicated, non-canonical, or above the per-call SSE wire
  ceiling.
- Provider HTTP/2 receive windows and frame size no longer float with reqwest
  defaults: initial stream and connection credit are each pinned to 65,535
  bytes, adaptive growth is off, and accepted frames are capped at 16,384
  bytes. Static mutation coverage protects each exact setting. This bounds
  protocol-level outstanding ingress while preserving the separate, honest
  exclusions for TLS/socket allocation and pinned Hyper's HTTP/1 receive buffer.
- Profile retention can no longer end from a copied lease alone. The bounded
  profile registry requires the exact supervisor cleanup receipt for that
  context. A never-started row must be registry-cancelled; an owned context must
  be destroyed or explicitly transferred to Browse; a borrowed tab must remain
  in Browse after release; and a sign-in handoff must be destroyed. Mismatched
  identities or terminal/resource substitutions leave the lease retained.
  Tests exercise both queued and active native-resource paths through the real
  supervisor. This closes the functional-core early-release path, not the
  remaining application actor, profile-directory, platform-store, deletion,
  concurrency, or migration qualification.
- Context shutdown sealing no longer discards queued authority. Never-started
  rows remain in the bounded registry after the admission seal, so the owning
  supervisor can cancel the exact assignment, release its run/node context
  budget, and produce the only receipt accepted by profile-lease release. The
  integration test seals both registries first, then proves they become
  quiescent only after supervisor cancellation and receipt-bound profile
  release. Active contexts still require native close and terminal reaping.
  This is functional-core shutdown ordering, not end-to-end application/native
  shutdown qualification.
- The stable native `AgentBrowserPort` can now linearize shutdown admission
  with a privacy-preserving resource audit. Tests race the seal against normal
  admission, fill the complete 16-task queue, reject outer dispatch,
  distinguish the shutdown settlement from ordinary audit settlements, and
  prove only bounded audits remain admissible for drain verification. The host
  reuses its validated native-resource accounting and the port adds no queue,
  worker, timer, page, or native object until it is taken. This closes the
  stable port race only.
- In unwind-capable development and evaluation builds, native-port ingress now
  catches panics at both the outer main-loop dispatch call and the later queued
  context/screenshot callback, seals admission, and reports one content-free
  fatal invariant. Tests distinguish pre-execution refusal (no terminal
  callback, both screenshot permits released) from a dispatcher panic after
  synchronous execution (the already-scheduled terminal remains exact). The
  release boundary mutation-tests both catch sites, their fail-stop
  transitions, and the poisoned-slot shutdown path that seals an already-taken
  port. Optimized desktop builds retain the workspace-wide `panic = "abort"`
  policy; this evidence does not claim recoverable teardown after a release
  panic.
- The functional core now consumes its sealed, empty context, profile-lease,
  cookie-transfer, action-execution, action-settlement, and screenshot owners
  before allowing the atomic port seal. The screenshot coordinator closes new
  capture admission without losing accepted terminal debt. One exact barrier
  audit plus at most seven strictly newer post-seal audits can produce a
  constructor-closed proof only after every validated native resource count is
  zero; mismatched events, replayed identities, nonzero snapshots, dispatch
  refusals, and attempt exhaustion cannot do so. This coordinator owns no port,
  clock, timer, task, worker, channel, browser object, or I/O. A concrete
  lifecycle implementation still must drive logical cancellation/native close,
  the real application event source, durable run audits, and policy authority.
- A stable terminal driver now consumes the coordinator only after its
  lossless admission has accepted the sealed cohort, so a readiness refusal
  returns every logical owner for continued cleanup. It drives the actual
  native port barrier under one caller-owned absolute deadline, accepts only
  the exact shutdown- and resource-audit event classes, uses checked strictly
  increasing identities, and waits between failed audits at a fixed 100 ms
  exponential cadence capped at one second and the coordinator's eight total
  attempts. It creates no worker, timer, channel, page, or native object, and
  its event diagnostics redact the complete payload. This closes the reusable
  imperative drain seam, not its concrete application/runtime composition or
  native-device qualification.
- A stable consuming lifecycle port now prevents an application integration
  from claiming clean agent shutdown without carrying the zero-resource proof.
  The dormant feature-gated Shell now owns and consumes that port before
  extension, Store, and engine teardown, returns it losslessly on every spawn
  failure through direct ownership flow rather than optional state and
  invariant aborts, contains panics, and applies the same ordering on
  unexpected actor exit. Worker-refusal and disconnected-handoff tests cover
  every rollback branch. This proves application control flow with fake
  lifecycles; no concrete runtime has been composed and no end-to-end native
  shutdown is claimed.
- The Browse named-device baseline intentionally has no values. No CPU, memory,
  GPU/compositor, energy, wakeup, or input-latency budget has been inferred.
- Checked-priced model receipts now preserve a content-free exact schedule
  digest, provider/billing class, catalog revision, and normalized
  cache/cache-write/reasoning subsets for reproducible aggregate metrics. No
  production catalog entry or live provider result is implied.
- Every policy-derived value admitted to supervisor progress now retains and
  rejoins the private canonical manifest-revision guard. Regression coverage
  rejects active operations, terminal receipts, permits, and human transitions
  minted from a different scope revision that deliberately reuses the same
  public manifest identity. The audit ledger independently rejoins that private
  revision on every current-progress snapshot, and the release boundary
  mechanically pins all six progress joins plus the audit admission.
- An optional run-local accounting reducer now aggregates exact model/effect
  receipts by usage-accounting, settlement, effect, proof, failure, opaque plan
  node, and bounded pricing-schedule digest. It independently enforces run/node
  budgets, accepts valid out-of-order concurrent settlement, and rejects
  replay or same-ID/different-revision substitution without partial mutation.
  It is an inert functional core with no application telemetry or persistence
  seam. It does not claim site, latency, native-resource, or machine-resource
  values.
- Default-core release gating now rejects three avoidable invariant-abort
  classes: rewrapping an already-proven nonzero semantic key, asserting static
  role-operation construction, and recovering a newly appended provider turn
  from a possibly-empty transcript. The replacement provider carrier reserves
  its eventual slot before binding, exposes the latest turn structurally, and
  merges it after serialization without a content copy or vector allocation.
  Unit tests preserve allocation/capacity and redaction facts; source
  mutations remove each carry or reintroduce `expect`, `unreachable!`, or the
  ordinary transcript type and are rejected. This narrows guarded defects
  under release `panic = "abort"`; it is not a general panic-recovery claim.
- The production macOS semantic channel likewise treats dispatch during its
  ordinary `Loading` lifecycle phase as a typed `NotReady` refusal. A
  bound-view/loading unit path and release source mutation gate replace the
  former duplicated-precheck `unreachable!()` assumption; no native execution
  or device behavior is claimed by that deterministic test.
- The full owned production inventory now carries a compile-time refusal for
  direct `unwrap`, `panic`, and `unreachable` findings: the agentic core,
  dormant provider transport, and all 16 dedicated engine agentic modules.
  Host all-feature/all-target Clippy passed for the two crates and engine graph;
  the two release-excluded Windows qualification binaries also passed their
  exact native-feature Clippy builds against the installed x86-64 MSVC target.
  Those cross-target builds emitted only the already documented shared
  target-CFG warning baseline. The Windows semantic protocol's three empty
  control commands now allocate their owned `"{}"` string directly under a
  compile-time size assertion instead of allocating a temporary JSON value and
  relying on `expect`. Unit and source-mutation tests pin the payload, response
  ceiling, constructor count, and abort refusal. This is compile/source
  evidence only; it does not qualify physical Windows behavior or establish a
  general release-panic-recovery guarantee.
- Both release-excluded physical-Windows runners, both native qualifier
  adapters, and their shared process/resource sampler now compile-deny direct
  `unwrap`/`expect`, `panic`, and `unreachable` findings outside tests. The
  boundary checker pins and mutation-tests all five attributes, while the
  native MSVC Clippy jobs enforce them on target. This makes explicit aborts a
  compile failure before evidence collection; it does not turn cross-target
  compilation into Windows behavior evidence or prove recovery from every
  possible panic source.
- A separate optional run-local progress reducer now streams canonical semantic
  audit events and derives observed initial-queue, model, effect, human-wait,
  and root elapsed durations, closed `NeedsHuman` counts, distinct human-
  takeover cancellations, and the root terminal outcome. Event/revision/time
  replay, skipped operation starts, an open model/effect at supervisor terminal,
  and invalid node sequences are rejected without partial logical mutation.
  Validated vector insertions and drained-cancellation matches carry their
  proven values into mutation rather than re-matching through an `unreachable!`
  process-termination branch; source-gate mutations pin both properties.
  Unobserved durations remain absent. The reducer retains no
  site/platform/resource labels and no sample distribution; exact medians and
  percentiles still require the qualification harness.
- A separate optional run-local action-performance reducer now borrows exact
  immutable batch terminals and derives complete/stopped/failed batch counts,
  verified and typed failed action counts, all three closed backend counts,
  settlement-event aggregates, and fixed 18-bucket distributions for native,
  settlement, native-to-settlement, and ordered proof-observation latency. It
  rejoins exact manifest/node authority, rejects replay and malformed terminal
  shapes transactionally, and cannot exceed the run operation budget. Its
  snapshot is compile-capped at 1 KiB and it owns no site/page label, raw
  sample, clock, task, telemetry, persistence, or browser/native resource.
- Exact provider disclosure now produces a copyable, at-most-64-byte input
  metrics value joined to the same committed input authority. It reports the
  serialized body bytes, closed observation/diff/locate/read/extraction or
  screenshot stats, newest semantic tokens when applicable, and exact complete
  structured-input tokens only for requests that actually ran that local
  counter. Initial request whole-input counts and screenshot semantic-text
  counts remain absent rather than inferred. Refused/cancelled transport has no
  public metrics surface, and the committed HTTPS attempt delegates the same
  value without retaining another page/image allocation.
- A separate optional run-local provider-input reducer now consumes only the
  exact copyable metric receipt minted by disclosure commit. It rejoins the
  private canonical manifest revision and rejects unknown nodes, duplicate
  call identities, contradictory closed source shapes, overflow, and run/node
  operation-budget excess without partial logical mutation. Its at-most-1-KiB
  snapshot reports aggregate exact request/disclosed bytes, semantic lines,
  measured-token sample/totals and quality counts, plus
  observation/diff/locate/read/extraction/screenshot source-shape and redaction
  facts. It retains no raw sample, content, site, model/tokenizer label, clock,
  task, telemetry, persistence, or native/browser resource; exact
  distributions still belong to the qualification harness.
- A separate optional terminal metric closure now refuses active or sealed
  supervisors, retained scheduler/context resources, incomplete audit node
  coverage, and root-outcome mismatch. It rejoins the private manifest
  revision across all four reducers; compares exact sorted model-call,
  effect-receipt, and native-attempt identities across their independent
  inputs; and requires matching progress-duration samples plus checked
  aggregate partitions. The copyable value is capped at 192 bytes, has no
  constructor other than the checked join, and is explicitly non-authorizing.
  It is not a run-settlement or production-qualification verdict: mutable
  policy, native resources, harness samples, sites, providers, and named
  devices remain separate required evidence.
- A subsequent clean-policy settlement now consumes the only mutable run
  policy after rejoining that exact closure and receipt-accounting scope. It
  requires an unsealed policy, zero pending/reserved debt, and exact run plus
  per-plan-node operation/token/cost reconciliation. The consuming call also
  requires its exact audit ledger to be shutdown-sealed, unambiguous, fully
  drained, and durably committed for every closure event. Every refusal returns
  both move-only owners, preventing pending authority or delivery debt from
  being discarded; success yields a redacted, non-authorizing value capped at
  256 bytes. Native resources and all named-device/site/provider qualification
  remain separate blockers.

The authoritative aggregate records and exact remaining blockers are in
`native-input-matrix-v1.json`, `semantic-runtime-macos-v3.json`, and
`browse-baseline-v1.json`.

## Application durable foundation (not live qualification)

The next 2026-09-05 slice adds optional `zephium-app/work-execution` admission
over the existing controller/runtime, and `zephium-store/work-execution` on the
existing Store actor. See the [persistence contract](../../docs/agent-work-persistence.md).
It requires exact durable admission before consuming the single-use native
factory and immutable terminal CAS before publishing success. Process-exit
fencing, restart interruption, review decisions and original-ledger audit
redelivery do not restore or replay native/provider work.

Deterministic evidence: 335 application tests (13 focused Work fixtures),
257 Store tests plus a subprocess helper exercised by its parent fixture,
417 default functional-core tests (515 with excluded localhost fixtures), 37 runtime tests, 13 production-feature
controller tests and 16 excluded controller/localhost tests, and 168 xtask
tests. The application fixture uses the actual shell command path and one real
SQLite Store for journal/audit/ordinary storage, but a typed synthetic native
port; it is not a live native workflow. Strict Clippy, optimized application,
Store/controller/default-desktop builds, content-free/source boundaries and
default-desktop dependency checks pass. Optimized probe enablement remains
compile-refused. No new live-provider or battery claim is made.

## Trusted macOS application composition evidence

The subsequent 2026-09-05 slice adds opt-in
`zephium-work-composition/macos-work` and `zephium-desktop/macos-work`. The
production factory takes the actual sealed EngineHost port only after the
existing application's two durable admission acknowledgements. Ordinary Store,
Work journal and audit share one exact `Arc<SqliteStore>`; the shell also checks
the exact engine allocation at attachment and prepared admission.

The excluded `--live-public-luna-work-application-inspectable` qualifier uses
that same composition with the actual shell, native engine, SQLite Store,
blocker/extension lifecycle owners and application shutdown. Only the provider
retention constructor is different. The explicit public Wikipedia task sets a
search value and Deutsch in either order, then refines the search value without
submitting. Trusted fresh-observation predicates determine completion; action
counts and model text do not. No ordinary tab or synthetic browser port is used.

Two corrected runs completed 2/2 tasks and 6/6 independently verified native
effects, with exact durable `Succeeded`, `ShutdownOutcome::Clean`, joined shell
and service owners, zero focus theft and exit 0:

| Run | Model turns | Verified effects | Input / output tokens | Priced ceiling (micro-USD) | Provider-turn time | Total elapsed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 4 | 3 | 11,151 / 357 | 785 | 10.182 s | 12.364 s |
| 2 (final build) | 4 | 3 | 11,228 / 398 | 846 | 10.938 s | 12.397 s |

The final build's four turns, in order (local counters only):

| Turn | Tool | Request bytes | New semantic bytes | Input / output tokens | Provider-turn time |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | Act | 12,000 | 3,116 | 2,512 / 122 | 3.813 s |
| 2 | Locate | 14,387 | 287 | 2,732 / 51 | 2.196 s |
| 3 | Act | 16,432 | 302 | 2,865 / 103 | 2.613 s |
| 4 | Act | 18,809 | 408 | 3,119 / 122 | 2.316 s |

An earlier run completed the task, durable success and clean application
shutdown but failed overall because the old qualifier wrapper attempted a
second one-shot engine shutdown. It is excluded from the passing count. The
wrapper now leaves successful application-owned teardown with the actual
application; host-only and failure paths retain bounded host cleanup. No
production shutdown proof or timeout was weakened. A four-case deterministic
ownership test covers that distinction.

Validation: 335 application, 459 native-probe engine, 257 Store, 417 default
functional-core, 13 production-feature controller, 37 runtime and 170 xtask
tests pass. The Store subprocess helper remains marked ignored for direct
invocation and is exercised by its parent test. The application fault matrix
now also rejects both foreign attached
engine owners and swapped prepared engine owners before native acquisition,
alongside its existing Store-identity, durable-write, mailbox, cancellation,
takeover, callback-loss and audit tests. Strict all-target Clippy passes for
application, native-probe engine, composition, desktop feature, qualifier and xtask; explicit
production-library Clippy and optimized desktop feature checks pass. Default
desktop optimized/dependency gates and semantic/controller/runtime source gates
pass. Enabling retained qualification in an optimized build is compile-refused.

This is the full shared application/native execution path, not a full Tauri
window/bootstrap or user-facing Work qualification. The qualifier's chrome
adapter refuses ordinary presentation; the selected isolated public profile has
explicit content policy and no user extensions. There is no UI/IPC task source
or new product task semantics. Production/BYOK remains `store:false`; only
these authorized public runs retain provider logs. No credentials, objectives,
page/provider contents, profiles or raw traces are committed. These small
repeats are not a latency benchmark, production readiness or battery claim.

Acceptance follow-up: desktop preparation now precedes attachment/claim and
slot consumption. Invalid or expired preparation preserves later valid
admission; a fresh absolute-deadline check occurs before attachment. Attachment
mailbox refusal retains the original prepared owner and composition, while
admission mailbox refusal retains the prepared owner and exact handle. A
deterministic slot-ordering regression, real stale-preparation/no-native/no-claim
fixture and adversarial source-order gate cover this change. The live figures
above remain evidence for the preceding build; this ordering correction does
not claim a new provider run. Follow-up validation passes 336 application tests,
171 xtask tests, the focused desktop admission regression, strict
desktop/composition/application/xtask Clippy, optimized desktop feature/default
builds and the same source/dependency boundaries.

## Production Work extraction and action-budget comparison (2026-09-05)

The production session, actor and durable application now deliver one bounded,
source-carrying extraction result through `take_extraction`. This is explicitly
model-mapped, memory-only user-result content, separate from diagnostics and
the content-free durable journal. No artifact-body persistence is claimed.
The [result contract](../../docs/agent-work-results.md) specifies exact schemas,
source joins, limits, cancellation and publication after durable terminal ACK.

The final public extraction task supplies a trusted schema for ten Wikipedia
language-link names. It independently compares all ten values with distinct
exact native source fragments before task completion; the application consumer
rechecks the owned result after durable success and verifies one-shot delivery.
The same actual composition, shell, EngineHost Work page, profile isolation and
shared SQLite Store are used. There are no native actions, ordinary tabs or
external writes in this extraction workflow.

Final-contract results, excluding earlier exploratory runs:

| Workflow / run | Provider turns | Independently checked result | Input / output tokens | Priced ceiling (micro-USD) | Provider-turn time | Elapsed |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| Extraction 1 | 2 | 10 exact cited values; 0 effects | 6,879 / 408 | 1,870 | 8.383 s | 9.007 s |
| Extraction 2 | 2 | 10 exact cited values; 0 effects | 6,878 / 368 | 1,822 | 7.147 s | 7.695 s |
| Action 1 | 4 | 3 verified effects | 11,149 / 379 | 1,086 | 9.884 s | 11.447 s |
| Action 2 | 4 | 3 verified effects | 11,226 / 377 | 820 | 10.400 s | 12.675 s |

All four runs used `gpt-5.6-luna`, reached the trusted task terminal and durable
`Succeeded`, proved clean application-owned teardown, preserved focus isolation
and exited 0. The action workflow remains variable: the first run chose locate
before fill, the second after fill; neither action count nor model text determined
success. Extracted source-edge counts were 11 and 14, including collection-level
citations; each item had one distinct exact native source. These are small public
qualification repeats, not a latency/resource benchmark or broad-site guarantee.

Final extraction per-turn disclosure:

| Run / turn | Request bytes | New semantic bytes | Input / output tokens | Provider-turn time |
| --- | ---: | ---: | ---: | ---: |
| 1 / 1 | 5,658 | 3,116 | 1,480 / 36 | 1.792 s |
| 1 / 2 | 21,160 | 13,330 | 5,399 / 372 | 6.591 s |
| 2 / 1 | 5,658 | 3,116 | 1,480 / 35 | 2.894 s |
| 2 / 2 | 21,160 | 13,330 | 5,398 / 333 | 4.253 s |

Final action per-turn disclosure (wall time is the settled-event clock):

| Run / turn | Tool | Request bytes | New semantic bytes | Input / output tokens | Provider-turn time | Wall time |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 / 1 | Locate | 12,012 | 3,116 | 2,516 / 85 | 2.287 s | 2.702 s |
| 1 / 2 | Fill | 14,184 | 208 | 2,661 / 95 | 2.172 s | 4.882 s |
| 1 / 3 | Select | 16,399 | 287 | 2,854 / 113 | 2.877 s | 8.153 s |
| 1 / 4 | Fill | 18,860 | 408 | 3,118 / 86 | 2.548 s | 10.956 s |
| 2 / 1 | Fill | 12,012 | 3,116 | 2,516 / 110 | 3.006 s | 3.397 s |
| 2 / 2 | Locate | 14,315 | 287 | 2,724 / 58 | 2.081 s | 6.748 s |
| 2 / 3 | Select | 16,400 | 302 | 2,864 / 107 | 2.344 s | 9.100 s |
| 2 / 4 | Fill | 18,821 | 408 | 3,122 / 102 | 2.969 s | 12.213 s |

Two evidenced defects were investigated rather than counted as success:

- The initial extraction mapping reached two priced calls but returned a type
  inconsistent with the trusted schema. Rust refused `Extraction(TypeMismatch)`;
  no result or clean success escaped. The previous provider envelope allowed all
  value kinds for every field. Its replacement binds exact trusted field names,
  kinds, schema identity and bounds before generation, while retaining independent
  Rust validation. Two intermediate corrected extraction runs also passed, but
  are not included in the final table.
- The first action regression stopped on the old collapsed `VerificationFailed`
  class. A detached accepted `4393585` control, changed only to retain the exact
  diagnostic reason and print content-free action metadata, reproduced a first
  Fill failure: `MutationQuiet(100 ms)`, 1,000 ms total action allowance, and
  `Verification(EvidenceAfterDeadline)` while awaiting the adjacent fresh native
  observation. This proves a pre-existing admission/timing defect independent of
  extraction; the original collapsed error cannot retroactively supply its exact
  subreason. Two diagnostic current-build action runs also passed before the fix,
  demonstrating why a small passing sample had not exposed the allowance defect.
  Two post-fix runs also passed with four and five turns while the qualifier's
  objective still requested the old allowance; the model followed the stricter
  schema. The final table uses two subsequent repeats after aligning that trusted
  objective with the admitted 2,000 ms capability.

The shipping snapshot driver now advertises and independently enforces a 2,000 ms
minimum **total allowance** before policy/native dispatch. Smaller proposals are
typed pre-dispatch refusals. This does not wait two seconds, extend an already
dispatched deadline, replay an action, weaken verification or alter WebKit's
attested `Throttle` scheduling. The 30 s hard action ceiling and original run
deadline remain. One final successful Fill took 1.267 s from `ActionActive` to
`Verified`, independently demonstrating that the previous one-second allowance
was insufficient even when the action and adjacent observation both completed.
The public [Apple policy contract](https://developer.apple.com/documentation/webkit/wkpreferences/inactiveschedulingpolicy-swift.enum)
defines limited processing, not a capture-latency guarantee; the chosen allowance
is based on this named-device observation, not a claimed platform-wide bound.

Deterministic validation passes 420 functional-core, 17 controller (including
eight extraction cases and seven native-action refusal cases), 337 application,
37 runtime, 257 Store and 459 native-semantic engine tests, plus 171 xtask tests.
The Store subprocess helper is ignored for direct invocation and exercised by
its parent test. Extraction cases cover valid ownership, schema substitution,
scope expansion, wrong type, forged citation, count/stream takeover and audit
loss. The native matrix includes undersized-budget refusal before dispatch and
exact postcondition refusal with its original charged effect retained. Result
tests cover source-span isolation, quote deduplication, observation teardown,
content-redacted diagnostics, phase-gated publication and one-shot delivery.
Existing crash/restart, immutable terminal CAS, uncertain writes, audit recovery,
cancellation, mailbox and lost-native-callback suites remain passing.

Strict all-target Clippy passes for core, shipping and fixture-enabled controller,
application, qualifier, composition, desktop feature and xtask. Optimized default
and `macos-work` desktop checks pass. Semantic/controller/runtime/model-catalog,
composition/persistence and default-dependency gates pass. The default desktop
graph has no controller, agent runtime, provider transport, Work composition or
reqwest dependency; its existing transport-free agentic core is unchanged as a
dependency. Optimized probe enablement is compile-refused. No manifest or lockfile
dependency changes, extra workers, Browse scheduling changes, secrets, objectives,
page/provider bodies, profiles, screenshots or raw traces are committed.

Only explicit retained-public qualification used provider storage; shipping/BYOK
remains `store:false`. OpenAI Logs remain exact provider-boundary evidence for
those public runs; local diagnostics carry only closed states, counters and
timings. Durable artifact delivery, navigation/multi-page tasks, native lifetime
reuse, parallel runs, UI/IPC task authoring, full Tauri bootstrap qualification,
and battery/CPU/RAM qualification remain open.

## Profile-owned atomic artifacts — 2026-09-05

The next bounded product vertical preserves useful Work results across process
exit without restoring execution. Trusted durable-profile extraction may opt
into a private artifact body committed atomically with its original successful
terminal. Memory-only remains the default. See the [artifact contract](../../docs/agent-work-artifacts.md)
for the 256 KiB/body, 32 MiB/Store, shared four-permit lane, exact profile and
process fencing, historical-data-only decoder and explicit refusal behavior.
Neither journal/audit rows nor diagnostic projections contain result bodies.

Three public Wikipedia Luna application attempts exercised the new artifact
path. All independently checked ten native-cited values and committed/reloaded
the artifact; only the corrected final attempt completed the entire qualifier.

| Attempt | Input / output tokens | Body bytes | Priced ceiling µUSD | Wall ms | Full outcome |
| --- | ---: | ---: | ---: | ---: | --- |
| Initial | 6,880 / 417 | 4,978 | 2,221 | 8,500 | Cleanup policy refusal |
| Second | 6,881 / 434 | 5,310 | 1,902 | 9,356 | Native cleanup refusal |
| Corrected | 6,879 / 407 | 5,296 | 1,869 | 8,713 | Complete |

The corrected run used two turns: 5,658/21,160 request bytes,
3,116/13,330 semantic bytes, and 2,858/5,186 ms model-turn elapsed. It proved
atomic artifact publication, independent archived-value/source checking,
native profile absence, Store finalization, durable `Succeeded`, clean Shell
shutdown and zero focus theft. There were zero mutating native actions; ten
cited values must not be reported as ten action effects. Only explicit
retained-public qualification used OpenAI storage; production/BYOK stays
stateless. No raw provider/page/body/crash data or test profile is committed.

Failures were investigated without retries or weaker proofs. Shell deletion
correctly requires Browse bootstrap, absent in this no-UI host. Excluded
cleanup now composes existing Store authorization, exact native namespace,
engine absence and Store finalization ports without bootstrapping an ordinary
tab. Cold recovery exposed WebKit's uninitialized main-run-loop enumeration
crash; the production adapter now initializes only a temporary configuration
before enumeration. The second refusal came from the excluded dispatcher's
autoreleased native objects: its pool covered run-loop pumping but not native
operations. Per-operation pools now match Cocoa dispatch. Both retained test
deletion obligations subsequently reached verified native absence and Store
finalization; their temporary directories were removed. No action was replayed.

Deterministic coverage includes four partial-write/ACK crash points, exact
immutable retransmission, profile deletion, quota/corrupt-body refusal, mixed
journal/artifact mailbox pressure, six actual controller/application publication
faults, read-only timeout/late-callback fencing, and a subprocess Shell/SQLite
exit after durable ACK but before consumer handoff. Its fresh reader creates no
provider worker/native factory and retrieves only archived data. Missing schema
refuses preparation; a trusted predicate that prematurely completes without
its promised result retains the original owner and cannot publish success or
claim clean shutdown.

Passing suites: 424 core, 17 controller, 342 application (one subprocess helper
ignored directly and exercised by its parent), 37 runtime, 263 Store (one
similarly exercised helper), 461 native-semantic engine plus three probe binary
tests, and 172 xtask tests. Optimized default and `macos-work` desktop checks
pass, as do 97 desktop tests (one existing ignored test). Strict all-target
Clippy passes for shipping and probe-enabled core/controller/application/engine,
runtime, Store, composition, qualifier, desktop and xtask. Semantic, controller,
runtime, model-catalog, composition/persistence and dependency gates pass;
optimized probe enablement is compile-refused. The default graph still excludes controller/runtime/provider transport,
Work composition and reqwest; no dependency/lockfile, idle worker, native
scheduling or action-protocol changes were introduced. General artifact
deletion/export, larger blob storage, new native lifetimes, multi-page work,
parallel admission, trusted task authoring/UI and resource/battery qualification
remain separate seams. This single corrected public workflow is not broad
production qualification.

### Parallel application-fixture deadline correction

Independent acceptance of the artifact pass exposed four `Deadline` failures in
the 338-test shipping-feature application suite. Healthy fixtures captured a
ten-second absolute run deadline before synchronous HTTP-client construction;
under parallel load, preparation's subsequent deadline recheck could correctly
reject that input. Healthy fixtures now use the existing transport-aligned
ten-minute production ceiling and a matching synthetic policy-clock expiry.
This changes no production check, serialization, explicit expiry test, or short
callback/pump/shutdown wait.

The exact `cargo test --locked -p zephium-app --features work-execution --quiet`
command passed six post-fix full-suite runs, including three launched together
(338 each). All-feature application tests passed twice (342 plus the existing
parent-exercised ignored subprocess helper), once at 32 test threads. Strict
shipping/all-feature Clippy, 172 xtask tests, all four agent architecture gates,
optimized default/`macos-work` desktop checks and the default dependency
exclusions passed. No live provider rerun was needed for this test-only fix.

## Fully settled unsuccessful Work closure — 2026-09-05

The actor now separates task failure/cancellation from resource drain. A
fully accounted refusal or stop can produce `ClosedUnsuccessfully`, then an
immutable zero-debt `Failed` or `Cancelled` fact after the original runtime
join and exact SQLite ACK. This removes needless unclean application shutdown
for ordinary settled failures without reopening the single-use native port.
Unknown callbacks, retained action/policy owners, mailbox faults and undelivered
audit still require recovery. No mutation is retried and no unsuccessful
result/artifact is published.

The excluded `--live-public-luna-work-cancellation-inspectable` mode uses the
same trusted macOS composition, actual Shell/SQLite application coordinator,
extension-free native context and Luna session as successful extraction. After
the first settled public tool turn it requests `HumanTakeover` through the
shipping application control port. It does not synthesize a provider/native
failure. Two runs passed, including the final rebuilt actor; the intervening
ordinary extraction regression also passed.

| Public application run | Charged input / output tokens | Priced ceiling µUSD | Wall ms | Exact full outcome |
| --- | ---: | ---: | ---: | --- |
| Human takeover | 1,480 / 37 | 415 | 3,402 | Cancelled, zero debt, Shell Clean |
| Successful extraction regression | 6,879 / 382 | 1,839 | 7,564 | Succeeded, verified result, Shell Clean |
| Human takeover, final build | 1,480 / 38 | 76 | 2,549 | Cancelled, zero debt, Shell Clean |

Each takeover run settled the first model turn (2,668/1,916 ms respectively),
then aborted and accounted the next extraction attempt (7/6 ms, zero charged
tokens). Request/semantic byte counts were 5,658/3,116 for the first turn and
21,184/13,330 for the aborted attempt. Both retained the exact `HumanTakeover`
cause, had no persistence error, returned no result and independently required
the durable `Cancelled`/zero-debt fact. They are not successful task completions.
The success regression used two completed turns (1,986/4,956 ms;
5,658/21,160 request bytes; 3,116/13,330 semantic bytes) and independently checked
ten native-cited values with fourteen source edges. All three runs had zero
native mutations/action effects, zero focus theft and clean application-owned
teardown. OpenAI retention was enabled only for this explicit public mode;
production/BYOK remains `store:false`. No content, credential, trace or profile
is committed.

Deterministic coverage includes settled provider count/stream refusal and
cancellation, the exact variable-turn ceiling, extraction contract refusal,
read/takeover/suspend/revocation races, refused/duplicate/lost native cancellation
and shutdown-audit receipts, and lost/refused durable audit delivery after native
drain. Original action refusal/callback/verification/charged-effect owners remain
in recovery. Failed terminal CAS tests cover foreign guards, lost ACKs, immutable
retransmission, late takeover and historical-read phase isolation. Actual
Shell/SQLite success and failure cases use separate processes: the real process
fence remains held even after clean shutdown. Core/Store tests cover immutable
failed/cancelled records, partial writes, rollback, post-commit uncertainty and
restart without executable restoration.

Passing final suites: 425 core; 14 shipping / 18 probe controller; 37 runtime;
339 shipping application and 343 all-feature application (the existing
subprocess helper is ignored directly and exercised by its parent), both
full-suite commands repeated; 264 Store plus four doctests (one similarly
parent-exercised helper); 461 semantic-native engine plus three probe tests;
97 desktop plus one existing ignored test; and 174 xtask tests. Strict Clippy
passes on shipping/probe controller and application, core, runtime, Store,
composition, qualifier, desktop and xtask, including shipping `--lib` checks.
Both optimized default and `macos-work` desktop checks pass. Controller,
runtime, catalog, composition/persistence and semantic source gates pass;
optimized qualifier enablement is compile-refused. The default desktop graph
still excludes controller/runtime/provider transport, Work composition and
reqwest. No dependency/lockfile, native scheduling, action protocol or idle
worker change was introduced; no new resource/battery measurement is claimed.

The single-attempt process/engine lifetime remains unchanged. Admission failure,
lost native action settlement and failure during an already-started terminal
resource close remain explicit recovery; this pass does not fabricate a second
close or a native resume operation. Task authoring, repeatable newly authorized
native lifetimes, richer navigation/tools, concurrent Browse and device-resource
qualification remain open. This is narrow full-application closure evidence,
not broad production qualification.

## Proof-gated sequential application/native lifetimes

The opt-in shipping composition now owns the process-unique native lifetime
factory. Each port permanently seals its own gate and sink. An explicit fresh
application admission may replace only its exact fully closed predecessor,
with the same engine/Store/process fence, original runtime/native/policy/audit
proofs, acknowledged immutable terminal and drained result/event lanes. Neither
a historical record nor an empty native audit alone grants a successor. See
[the ownership decision](../../docs/agent-work-lifetimes.md). This supersedes the
single-attempt process limitation above; uncertain predecessors still block.

The excluded `--live-public-luna-work-sequential-inspectable` qualifier exercises
two newly authorized public Wikipedia runs in one actual native engine,
application Shell and SQLite Store. Human takeover closes the first run after
one real Luna tool turn. A fresh task/context/manifest then extracts a result
using the original composition's newly issued native lifetime. The qualifier
sends a stale takeover through the old handle while the successor is Running;
the old projection cannot control the new run. Both sequences passed, including
the rebuilt native audit-invalidation boundary.

| Sequence | Cancelled predecessor input / output | Successful successor input / output | Total priced ceiling µUSD | Whole sequence ms | Independently checked successor |
| --- | ---: | ---: | ---: | ---: | --- |
| Initial | 1,480 / 36 | 6,883 / 461 | 2,349 | 11,331 | 10 values / 14 source edges |
| Audit-invalidation build | 1,480 / 36 | 6,879 / 375 | 1,905 | 8,966 | 10 values / 11 source edges |

Each predecessor retained exact `HumanTakeover`, immutable `Cancelled`, zero
debt and no result; its closure took 3,211/2,438 ms. Successor records were
distinct, the original terminal remained unchanged and the stale control was
ignored. Both successors required the trusted extraction predicate and exact
native source validation; neither relied on model completion text. All four
lifetimes had zero native mutations/action effects. Both sequences had zero
focus theft and one final application-owned Clean Shell shutdown. This proves
serial native reuse by new ports, not concurrent execution, mutation replay,
normal-tab borrowing or automatic recovery.

The completed predecessor model turns took 2,489/1,778 ms; the next attempts
were aborted and accounted in 9/10 ms with zero charged tokens. Successor turns
took 1,688/6,126 ms and 1,601/4,620 ms. Every first turn used 5,658 request bytes
and 3,116 semantic bytes. Second attempts used 13,330 semantic bytes and
21,160 request bytes, except the initial successful successor's 21,204 bytes.
Aggregate charged usage was 16,722 input / 908 output tokens, 4,254 µUSD priced
ceiling, and 20,297 ms whole-sequence wall time. Explicit public qualification
alone retained OpenAI logs; production/BYOK remains stateless. No page, model,
credential, profile, screenshot or raw trace is committed.

Deterministic native tests cover exclusive legacy/factory acquisition, distinct
ports, permanent old seals/sinks, the 1,024-lifetime ceiling, pending task and
physical capture ownership, mismatched/refused/lost/nonempty audit receipts,
sticky lineage failure, and retirement versus audit/global-shutdown races.
Read-only audit uncertainty invalidates an earlier shutdown certificate before
permit release; a subsequent ordinary audit cannot manufacture another one.
Application fixtures drive actual Shell/SQLite success→success and settled
failure→success without releasing the process fence. They reject stale/wrong
engine/Store predecessors, pending terminal ACK/event/result/archive owners,
ordinary second attachment, saturated mailbox and missing original proofs.
Lost callback and recovered audit debt remains non-replaceable. Existing
partial-write/restart/approval-CAS tests remain unchanged.

Validation: 425 core; 14 shipping and 18 probe controller; 37 runtime; 339
shipping and 343 all-feature application; 264 Store plus four doctests; 465
shipping native engine and 467 semantic-native engine plus three probe tests;
97 desktop; 175 xtask tests. Existing ignored subprocess helpers remain
parent-exercised. Full application and controller suites were repeated. The
broader concurrent sweep exposed the controller fixture's old ten-second
healthy deadline including synchronous client construction. Healthy fixture
input and synthetic policy expiry now use the existing ten-minute hard ceiling;
explicit missing-audit/observation expiry retains its short post-construction
deadline, and worker/pump waits are unchanged. No production deadline changed.

Strict Clippy, source/semantic/controller/runtime/catalog gates, optimized
default and opt-in desktop checks pass; optimized qualifier builds are refused.
The default desktop graph still excludes Work composition, controller, runtime,
provider transport and reqwest. No default feature, dependency, native scheduling,
worker, idle poll or lockfile change was added. This is structural evidence,
not a new resource/battery/endurance benchmark. Unknown action/callback/audit
debt, failed admission and failed already-started resource close still require
explicit recovery; parallel contexts/runs need a future scoped proof design.
Trusted task authoring, richer tools, full desktop bootstrap and broader
platform/site/resource qualification remain open.

## One application run: verified actions to a cited result (2026-09-05)

The shipping Work actor now admits an explicitly trusted combined task. This
removes the previous mutual exclusion between snapshot actions and useful
schema-bound output: the same session can prepare state and return one cited
result. The task freezes its schema and combined mode before execution,
evaluates readiness from fresh native observations, and independently accepts
the mapped result. No model text, action count or new worker supplies authority.

The exact locate/act/extract capability profile remains continuation-bound in
both provider serializers. Read-only extraction remains the default for a
schema task. Premature extraction and further actions after trusted readiness
fail closed, without re-prompting or replay. Locate, verified actions, extraction
proposal and mapping share the existing eight-call ceiling. The mapping binds
the current post-action observation, frame cohort and capture timestamp; it
cannot substitute the initial action baseline. Original policy/accounting,
native settlement, runtime, durable audit and terminal-publication proofs remain
required before one-shot result handoff.

The release-excluded command
`--live-public-luna-work-combined-inspectable` exercised the actual macOS
composition, shell, EngineHost-owned extension-free page and shared SQLite
Store. The trusted public-test task required the existing three search-field
and language milestones, without submission or external writes, followed by
one exact final-field value and its complete native value-preview citation.
Both the trusted task and the application result consumer independently checked
that evidence. No task definition or special authority was added to production.

Two runs passed, including the final rebuilt qualifier. Both produced the
sequence Fill → Locate → Select → Fill → Extract → mapping, with three native
effects independently verified before result mapping. Each reached durable
Succeeded, delivered its result once, passed focus isolation and completed
application-owned Shell Clean teardown. No action, provider or verification
retry occurred. These runs used ephemeral profile storage and memory-only result
bodies; durable terminal facts were acknowledged. They do not add live evidence
for combined-task artifact restart retrieval.

| Run | Input / output tokens | Accounted µUSD | Provider-turn ms | Total wall ms | Verified effects / results |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 21,136 / 465 | 3,184 | 15,128 | 16,591 | 3 / 1 |
| 2, final build | 21,338 / 533 | 2,700 | 14,128 | 16,472 | 3 / 1 |
| Aggregate | 42,474 / 998 | 5,884 | 29,256 | 33,063 | 6 / 2 |

Final-build per-turn boundary receipts (cost accounting is `PricedCeiling`):

| Turn | Kind | Input / output tokens | Request bytes | Semantic bytes | Provider-turn ms | µUSD |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | Fill | 2,613 / 124 | 12,794 | 3,116 | 2,812 | 202 |
| 2 | Locate | 2,835 / 89 | 15,201 | 287 | 2,944 | 216 |
| 3 | Select | 2,984 / 80 | 17,398 | 208 | 2,052 | 191 |
| 4 | Fill | 3,215 / 113 | 19,667 | 408 | 2,348 | 254 |
| 5 | Extract proposal | 3,428 / 41 | 22,058 | 299 | 1,916 | 168 |
| 6 | Schema mapping | 6,263 / 86 | 30,603 | 13,365 | 2,056 | 1,669 |

The current initial-scope mapping still discloses a bounded read larger than this
single result requires; this pass does not claim targeted-read efficiency. The
explicit public qualification mode alone retained provider logs. Production and
BYOK stay `store:false`; local receipts contain only typed stages, correlations,
counts and timings. No page/provider bodies, credentials or profile state were
retained in repository evidence.

The combined controller fixture runs fifteen success/fault schedules through
the original runtime and localhost provider/native ports: success, premature
extraction, late action, wrong schema, expanded scope, foreign source, mapping
count/stream takeover, lost audit/native callback, schema/mode mutation, false
completion without a result, trusted result refusal and the exact eight-call
ceiling. Missing-schema admission/readiness also fail without provider calls.
Already verified effects stay accounted on refusal; lost original owners retain
Recovery, and a valid result with missing audit remains unpublished. No clean
proof is synthesized. Source-gate mutation tests protect the frozen task phase,
current evidence join and independent application qualifier checks.

Validation: 425 core; 15 shipping and 20 probe controller; 37 runtime; 339
shipping and 343 all-feature application; 264 Store plus four doctests; 465
shipping and 467 semantic-native engine plus three native probe tests; 97 desktop,
two qualifier and 177 xtask tests. Ignored subprocess helpers remain
parent-exercised. Strict Clippy and the controller/runtime/catalog/semantic,
composition and persistence gates pass. Optimized default and opt-in `macos-work`
desktop checks pass; the optimized qualifier is rejected by its compile-time
boundary. The default desktop graph still excludes Work composition, controller,
runtime, provider transport and reqwest. No dependency, default feature, lockfile,
native scheduling, worker or idle-poll change was made. This is not battery,
RAM/CPU, full-desktop-bootstrap or broad-site production qualification.

The next larger execution seam is trustworthy cross-document navigation and
its effect authorization, settlement and fresh continuation proof. Model-facing
navigation vocabulary alone does not supply that host adapter. Trusted task
authoring, richer read scopes, parallel runs and UI remain separate product work;
this pass neither enables nor emulates them.

## Exact native subtree extraction (2026-09-05)

The accepted action-to-result path disclosed 13,365 semantic bytes for one
field. The next bounded vertical adds actual native subtree capture, not a
filtered copy of the initial observation. Trusted tasks explicitly enable it;
default schema tasks remain initial-only and read-only. The same actor, runtime,
browser/context port, provider session, policy, audit, Store and publication
owners remain in control. No new worker, queue, native program or default
dependency is introduced.

The settled extraction proposal's opaque target is validated against its exact
model-acknowledged predecessor and frame cohort. The native expansion runs once
under the existing capture bounds. Read admission independently requires the
returned root's private stable node key to match the selected anchor, rejecting
an accidentally widened full-frame result. A distinct read guard binds predecessor,
target and fresh source; policy requires the original committed anchor taint
cohort and adds only fresh read-guard taint. It never acknowledges the expanded
observation to the model as an action baseline, and terminal mapping cannot
create another tool continuation. Scope/config/schema substitutions, stale
native capture and uncertain original owners fail closed without replay.

The excluded `--live-public-luna-work-scoped-inspectable` command uses the full
application composition, real EngineHost-owned page and exact shared SQLite
Store. It performs the existing three unsubmitted public Wikipedia form-state
milestones, then extracts the final value from a fresh subtree rooted at the
acknowledged search field. Both task acceptance and application delivery
independently compare that value with its untruncated native citation. No
production task, probe authority, private account or external write is added.

The first attempt verified all three effects and captured the subtree, then
failed before mapping with `Browser(Authority)`: extraction policy still
required an initial-read acknowledgement and baseline-only taints. It correctly
persisted failure, withheld output and reached original Shell Clean teardown.
That attempt used 15,482 input / 479 output tokens, 1,704 accounted µUSD and
18,721 ms. The fix carries the exact selected target through the sealed draft
and checks fresh read provenance at policy admission; it does not waive the
initial-read boundary or mint action authority. This failed attempt is not
included in successful-run aggregates.

Three corrected runs passed, including the final exact-root build. All used
Fill → Locate → Select → Fill → scoped Extract → mapping, reached durable
Succeeded, returned one result once, passed focus isolation and completed
application-owned Shell Clean teardown. No action/provider/native retry occurred.

| Run | Input / output tokens | Accounted µUSD | Provider-turn ms | Total wall ms | Verified effects / results |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 17,854 / 482 | 1,685 | 16,391 | 18,650 | 3 / 1 |
| 2, rebuilt repeat | 17,904 / 469 | 1,677 | 13,805 | 15,487 | 3 / 1 |
| 3, final exact-root build | 17,947 / 507 | 1,736 | 15,903 | 17,678 | 3 / 1 |
| Aggregate | 53,705 / 1,458 | 5,098 | 46,099 | 51,815 | 9 / 3 |

Final-build boundary receipts (`PricedCeiling` accounting):

| Turn | Kind | Input / output tokens | Request bytes | Semantic bytes | Provider-turn ms | µUSD |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | Fill | 2,705 / 99 | 13,280 | 3,116 | 3,662 | 174 |
| 2 | Locate | 2,902 / 55 | 15,539 | 287 | 2,422 | 170 |
| 3 | Select | 3,039 / 89 | 17,614 | 302 | 2,644 | 200 |
| 4 | Fill | 3,279 / 125 | 19,927 | 408 | 2,779 | 272 |
| 5 | Scoped extract proposal | 3,504 / 59 | 22,382 | 299 | 2,079 | 194 |
| 6 | Schema mapping | 2,518 / 80 | 17,611 | 586 | 2,317 | 726 |

All scoped mappings disclosed 586 semantic bytes, 95.6% below the previous
initial-scope mapping's 13,365 bytes. Final mapping input was 2,518 versus 6,263
tokens (59.8% lower). These are measured disclosure/input differences, not a
controlled provider-latency or cost benchmark; caching and generation vary.
The extra native capture completed in 3–5 ms across the corrected runs.

Eighteen controller schedules use the actual worker and synthetic provider/native
ports: read-only and combined success with newly captured content absent from
initial observations; unknown ref, wrong schema, absent grant, refused/stale/lost
capture, widened/wrong-root response, takeover, renderer loss, mapping count/stream
cancellation, foreign citation, audit loss, contract mutation, unauthorized action
and exhausted mapping budget. The final budget precheck refuses an extra capture
when no mapping turn remains; it is covered deterministically and does not change
the six-call live path. Core tests cover exact predecessor, frame, generation,
target, config, schema, policy anchor/origin and committed
source/output bindings for both provider protocols. Lost capture retains its
original correlation; audit loss retains unpublished output; renderer loss
retains recovery. No fixture manufactures a clean closure. Source mutation gates
protect those proof joins and the frozen product capability.

Broader feature-unified tests also exposed two old transport-fixture assertions
that prohibited the literal `title` anywhere in the output schema. Trusted field
binding intentionally includes that field-name enum. The assertions now verify
the exact trusted schema, closed field, type and bounds for both transports;
no production output format changed for this repair.

Public runs used ephemeral profiles and memory-only result bodies with durable
terminal facts. They qualify targeted fresh capture and disclosure reduction,
not public off-viewport discovery, artifact restart, multi-page research,
parallel execution or full desktop bootstrap. Newly disclosed subtree content
is covered deterministically, not claimed from these public search-field reads.
No battery/CPU/RAM/endurance or broad-site/platform claim is added. Production
and BYOK stay `store:false`; only explicit public probes retain provider logs.
Local evidence contains no page/provider bodies, objectives or credentials.

Validation: 426 no-transport core tests; 524 feature-enabled core tests plus
three native-action and five semantic-runtime integration tests; 15 shipping
and 21 probe controller tests (the scoped matrix contains eighteen schedules);
37 runtime; 339 Work application and 343 all-feature application; 264 Store
plus four doctests; 465 engine; 97 opt-in desktop; two qualifier; 178 xtask.
Ignored subprocess helpers remain parent-exercised. Strict Clippy, controller,
runtime, catalog, semantic/native, composition and persistence source gates pass.
Optimized default and opt-in desktop checks pass; optimized probe compilation
is refused. The default desktop dependency tree still excludes Work composition,
controller, runtime, provider transport and reqwest. No manifests, dependencies,
lockfile, native scheduling or idle polling changed.

One overlapping build/test validation run hit the existing twelve-second actor
fixture watchdog in the lost-callback group. The old diagnostic did not identify
the individual fault. That watchdog now includes only typed fault/callback codes;
the isolated group and two successive full actor sweeps passed without changing
the watchdog or any production deadline. This observation is not claimed as a
root-caused runtime repair or endurance qualification.

Cross-document navigation still needs its own trusted effect subject,
authorization, settlement and new-document continuation proof; bare lifecycle
navigation is not a safe substitute. Task authoring, richer read scopes and
parallel runs remain separate work.

## Trusted local form task qualification (2026-09-05)

Implementation `54d70c0` replaces the public application qualifier's bespoke
action predicate with the shipping `AgentWorkFormTask`. Explicit trusted
field/value phases now supply completion and fresh-target assessment through
the same durable application coordinator, shared Engine/Store identities,
owned extension-free page, controller, policy, native verification and terminal
owners. There is no new browser/orchestration path. The local-only effect
attestation is explicit: this contract does not infer that arbitrary Fill or
Select controls lack autosave or other remote effects. See
[trusted form contracts](../../docs/agent-work-forms.md).

Two final-build public macOS/WKWebView application paths passed with
`gpt-5.6-luna`, explicit retained-public OpenAI logging and content-free local
diagnostics. They used ephemeral isolated profiles, non-submitted public
Wikipedia form preparation and memory-only result content:

| Final path | Turns | Verified effects | Cited results | Input/output tokens | Cost µUSD | Provider-turn ms | Wall ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Action-only | 4 | 3/3 | Not requested | 11,272 / 396 | 1,426 | 11,422 | 12,859 |
| Action → exact subtree result | 6 | 3/3 | 1/1 | 17,941 / 524 | 1,747 | 17,249 | 18,377 |

Both reached immutable durable `Succeeded`, original application-owned clean
teardown and passing focus isolation. The result path's separate trusted
consumer independently checked the exact cited current field value. The
aggregate is 29,213 input and 920 output tokens, $0.003173, 28,671 ms provider
turn time and 31,236 ms wall time. These are two distinct path samples, not
repeatability/latency-distribution, broad-site or resource/battery qualification.
No retries, navigation, submission or external writes occurred. Production/BYOK
requests remain `store:false`.

Final per-turn provider-boundary measurements:

| Path/turn | Tool | Input | Output | Request bytes | Semantic bytes | µUSD | Turn ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Actions 1 | Act | 2,516 | 128 | 12,012 | 3,116 | 783 | 3,970 |
| Actions 2 | Locate | 2,742 | 52 | 14,483 | 287 | 170 | 2,284 |
| Actions 3 | Act | 2,876 | 111 | 16,524 | 302 | 223 | 2,657 |
| Actions 4 | Act | 3,138 | 105 | 18,965 | 408 | 250 | 2,511 |
| Result 1 | Act | 2,705 | 114 | 13,280 | 3,116 | 192 | 3,575 |
| Result 2 | Locate | 2,917 | 71 | 15,647 | 287 | 193 | 2,528 |
| Result 3 | Act | 3,048 | 93 | 17,748 | 208 | 204 | 3,070 |
| Result 4 | Act | 3,292 | 94 | 20,081 | 408 | 236 | 3,585 |
| Result 5 | Extract | 3,486 | 52 | 22,320 | 299 | 178 | 1,940 |
| Result 6 | Mapping | 2,493 | 100 | 17,505 | 586 | 744 | 2,551 |

An earlier development result-path run also passed 3/3 effects and 1/1 result
with durable success, clean teardown and focus isolation: 17,889 input / 507
output tokens, 1,725 µUSD, 15,745 ms provider turn time and 18,286 ms wall. It
preceded the explicit initial-scope admission guard and constructor naming;
it is not included in the final-build aggregate above. Development fixture
errors were impossible operation masks/already-satisfied action construction
and a source-gate test import; production checks were not relaxed to fix them.

Deterministic contract coverage includes both action orders, skipped phases,
wrong values/options/targets/effects, stale observation IDs/generations,
foreign context/origin, exact value truncation, ambiguous/missing fields and
options, secret/disabled/read-only controls, incomplete and partial scopes,
bounded/overlapping configuration, explicit clear and zero-action completion.
Any evidence refusal clears retained bindings and prevents reuse. Thirteen
existing-worker/loopback/native fault schedules exercise success, wrong-value
refusal before native dispatch, approval-required policy refusal, insufficient
settle budget, dispatch/callback/verification failure, human takeover, lost
native callback, provider count/stream failure and count/stream cancellation.
They assert the original pending/failed action, policy, callback and recovery
owners. Two shipping-worker cases prove initial completion/missing-field refusal
uses zero provider calls and zero native effects with honest clean closure.

Validation: 22 shipping and 29 probe controller tests; 524 core tests plus three
native-action and five semantic-runtime integration tests; 339 Work application
and 343 all-feature application; 264 Store plus four doctests; 37 runtime;
97 opt-in desktop; two qualifier; 179 xtask. Ignored subprocess helpers remain
parent-exercised. Strict controller/application/Store/composition/xtask,
qualifier and opt-in desktop Clippy pass. Controller, runtime, model-catalog,
semantic/native, composition and persistence architecture gates pass, including
the new exact form value/ref/freshness and content-sink mutation gates.
Optimized default and opt-in desktop checks pass; optimized probe compilation
is refused. The default desktop dependency tree excludes Work composition,
controller, runtime, provider transport and reqwest. No manifest, lockfile,
native scheduling, provider schema, persistence format, queue or idle polling
changed. No page/provider body, objective, credential or raw trace was recorded
in this local evidence.

Trusted general plan admission, reviewed site/effect contracts and richer
cross-document work remain separate product seams. This task does not restore
goals/phase position/refs after restart or authorize mutation replay. The
existing interrupted-run/recovery and new-admission barriers remain unchanged.

## Remaining release blockers

- authorized named-device macOS native-input routes beyond fixed DOM;
- the closed physical-Windows seven-mode semantic/lifecycle matrix, including
  the dedicated native same-document replacement/rejoin case;
- one separately authorized difficult real-site run per platform;
- named macOS and Windows Browse startup, idle, tab-pressure, and concurrent-use
  baselines plus reviewed acceptable agent deltas;
- retained action/run timing samples and reviewed per-site latency
  distributions from the qualification harness (the local action reducer
  retains only fixed content-free histograms without site, device, or platform
  labels);
- reviewed committed semantic-input size, redaction, and token distributions
  from authorized provider/tokenizer qualification (the stable content-free
  commit and aggregate seams exist, but the reducer deliberately retains no
  raw distribution and this repository provides neither a telemetry sink nor
  a live-provider result);
- deterministic semantic/action/policy suite closure, the six-site matrix,
  concurrent production configuration, endurance, and fault-injection evidence;
- hot-path and zero-unused-agent-overhead measurements; and
- completion of native/unsafe, secret, and log review outside the scoped
  agentic-owned production modules and dormant provider transport, plus
  application/platform profile integration, composition of the higher
  cancellation lifecycle with the provider-transport shutdown proof and
  terminal provider usage/policy/audit settlement, a real application event
  source, end-to-end native use of the bounded zero-resource proof, recovery,
  migration, and stable Work-port audits beyond the dormant Shell seam.

None of these pending items is represented as zero, passing, or non-blocking.
