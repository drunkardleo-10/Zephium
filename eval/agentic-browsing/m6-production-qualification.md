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
  `semantic-runtime-macos-v1.json`, and `capabilities-v1.json` with closed
  schemas and file-size ceilings;
- rejects unknown fields, machine-local paths, and high-confidence secret or
  raw-content fields;
- requires the named-device Browse baseline to remain empty while its status is
  pending;
- binds the reviewed macOS hidden fixed-DOM aggregate to its exact command,
  platform, backend, case count, trust/focus/activation results, and teardown
  result;
- binds the reviewed macOS production semantic aggregate to its exact command,
  OS/WebKit build, closed viewport, two snapshots/world epochs, bridge,
  redaction, focus, teardown, and explicit non-claims;
- prevents the cross-compiled Windows runner from being represented as
  device-qualified without a reviewed gate change;
- cross-checks the Wry, Tauri, and `tauri-runtime-wry` capability versions and
  revisions against their vendored manifests and provenance files; and
- continues to prove that diagnostic features and the dormant provider HTTPS
  transport are absent from the ordinary desktop release graph; and
- independently compile-refuses the fixed macOS semantic qualifier in
  optimized builds and proves it absent from that graph.

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
cargo xtask check-agentic-probe-boundary
cargo clippy --locked -p xtask --all-targets -- -D warnings
```

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
  not physical-device behavioral evidence.
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

The authoritative aggregate records and exact remaining blockers are in
`native-input-matrix-v1.json`, `semantic-runtime-macos-v1.json`, and
`browse-baseline-v1.json`.

## Remaining release blockers

- authorized named-device macOS native-input routes beyond fixed DOM;
- the closed physical-Windows four-mode matrix;
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
- completed native/unsafe, profile, secret, log, shutdown, recovery, migration,
  and stable Work-port audits.

None of these pending items is represented as zero, passing, or non-blocking.
