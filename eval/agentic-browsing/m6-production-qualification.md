# Milestone 6 production qualification

Status: in progress; Zephium is not yet production-qualified for agentic
browsing.

This is the reproducible engineering record for Milestone 6. It reports only
reviewed aggregate evidence and points to executable gates. It must not contain
raw probe JSONL, page content, screenshots, profiles, credentials, provider
responses, machine-local paths, or native traces.

## Mechanically enforced evidence

`cargo xtask check-agentic-probe-boundary` is part of ordinary CI and now:

- decodes `browse-baseline-v1.json`, `native-input-matrix-v1.json`,
  `semantic-runtime-macos-v2.json`, and `capabilities-v1.json` with closed
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
  OS/WebKit build, closed viewport, four snapshots across three world epochs,
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
  across each helper-worker refusal and disconnected actor handoff. The Shell
  consumes agent shutdown after retryable durability preflight and before
  extension, terminal Store, and engine teardown; `Clean` joins the proof,
  while unclean results and panics still run every later barrier. The same
  consuming cleanup is mandatory on unexpected actor exit.

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
  macOS 27.0 build 26A5421a / WebKit 22625.1.29.11.25. One hidden,
  extension-free, ephemeral 1280-by-800 logical view produced two checked
  snapshots across two isolated-world epochs; page-world bridge exposure and
  focus theft remained absent and all retained native owners drained. The
  closed manifest explicitly excludes arbitrary-site, Windows, provider-token,
  and Browse/resource claims.
- The Windows adapter is source-guarded and cross-compiles. Cross-compilation is
  not physical-device behavioral evidence. Its owned-context port now admits
  hidden suspend/resume through WebView2 only, with a ten-second UI-thread
  deadline, exact callback/timeout/cancellation ownership, retained late
  reconciliation, final `IsSuspended` readback, content-policy exclusion, and
  resource-audit refusal while physical state is ambiguous. macOS remains
  unsupported at this port. No suspend/resume device or resource claim is made.
- The physical Windows M1 reviewer now refuses a success label without the
  case-specific effect event on the exact intended target, refuses any retained
  effect for a non-dispatch route, and cross-checks link, clipboard, and popup
  fields against the claimed terminal outcome. The release boundary
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
  three coarse focus-owner samples to match the selected presentation and the
  independently observed target DOM-focus bit; clearing the summary theft
  Booleans cannot hide a contradictory serialized focus sequence. After the
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
- The physical Windows semantic source gate now binds its content-free success
  fields to the closed snapshot verifiers, exact flood and renderer-loss
  refusals, native suspend/readback/resume result, semantic drain audit, and
  teardown result. Mutation tests replace each evidence class with an
  unconditional label and prove release qualification fails. This is producer
  integrity coverage, not physical Windows behavior evidence.
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
  failure, contains panics, and applies the same ordering on unexpected actor
  exit. This proves application control flow with fake lifecycles; no concrete
  runtime has been composed and no end-to-end native shutdown is claimed.
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
- A separate optional run-local progress reducer now streams canonical semantic
  audit events and derives observed initial-queue, model, effect, human-wait,
  and root elapsed durations, closed `NeedsHuman` counts, distinct human-
  takeover cancellations, and the root terminal outcome. Event/revision/time
  replay, skipped operation starts, and invalid node sequences are rejected
  without partial logical mutation. Unobserved durations remain absent. The
  reducer retains no site/platform/resource labels and no sample distribution;
  exact medians and percentiles still require the qualification harness.
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
`native-input-matrix-v1.json`, `semantic-runtime-macos-v2.json`, and
`browse-baseline-v1.json`.

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
