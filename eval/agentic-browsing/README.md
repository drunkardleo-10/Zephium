# Agentic browsing evidence

This directory contains reviewable, non-sensitive manifests for the browser
execution proof. The Rust contract in `zephium-agentic` is the wire-schema
authority. These files describe exact pinned capabilities, named-device
measurement procedures, and the absence or presence of reviewed results.

Committed evidence must never contain a profile location, username, account,
cookie, credential, token, authorization header, page text, HTML, screenshot,
provider response, native trace, or raw error string. Device records use only a
reviewed hardware class, OS/runtime versions, and aggregate resource counters.

Deterministic contract and fixture checks:

```sh
cargo test --locked -p zephium-agentic --features probe-harness
cargo clippy --locked -p zephium-agentic --all-targets \
  --features probe-harness -- -D warnings
cargo test --locked -p zephium-agent-provider-transport
cargo clippy --locked -p zephium-agent-provider-transport \
  --all-targets -- -D warnings
cargo test --locked -p zephium-engine \
  --features native-agentic-input-probe --lib
cargo test --locked -p zephium-engine \
  --features native-agentic-input-probe \
  --bin macos-agentic-input-probe
cargo clippy --locked -p zephium-engine \
  --features native-agentic-input-probe \
  --bin macos-agentic-input-probe --lib -- -D warnings
cargo test --locked -p zephium-engine \
  --features agentic-browser --lib
cargo clippy --locked -p zephium-engine \
  --features agentic-browser --all-targets -- -D warnings
cargo check --locked -p zephium-engine \
  --features native-agentic-semantic-probe \
  --bin macos-agentic-semantic-probe
cargo check --locked --target x86_64-pc-windows-msvc \
  -p zephium-engine --features native-agentic-input-probe \
  --bin windows-agentic-input-probe
cargo clippy --locked --target x86_64-pc-windows-msvc \
  -p zephium-engine --features native-agentic-input-probe \
  --bin windows-agentic-input-probe
cargo check --locked --target x86_64-pc-windows-msvc \
  -p zephium-engine --features agentic-browser
cargo check --locked --target x86_64-pc-windows-msvc \
  -p zephium-engine --features native-agentic-semantic-probe \
  --bin windows-agentic-semantic-probe
cargo clippy --locked --target x86_64-pc-windows-msvc \
  -p zephium-engine --features native-agentic-semantic-probe \
  --bin windows-agentic-semantic-probe
cargo xtask check-agentic-probe-boundary
```

The default `zephium-agentic` tests also exercise the shipping Milestone 2
context/profile/cookie/handoff core, the initial Milestone 3 semantic runtime,
and the bounded Milestone 4 action/read/extract/screenshot functional core.
They require no native view, worker, network listener, profile data, provider
call, or diagnostic feature. See `m2-context-identity.md`,
`m3-semantic-runtime.md`, `m4-action-pipeline.md`, and
`m5-policy-supervisor.md` for the exact implemented and still-pending
boundaries.

An optimized build with `probe-harness` is expected to fail at compile time.
The ordinary `zephium-desktop` resolved graph may reach the production
functional core through Store, but is separately required not to activate
`probe-harness`, `native-agentic-input-probe`, or
`native-agentic-semantic-probe`. All probe-only contract,
control, evidence, protocol, recipe, fixture, and native modules remain behind
those features. The same source gate pins the provider transport's two exact
HTTPS endpoints, rustls/system-proxy feature graph, redirect/retry refusal,
sensitive credentials, identity encoding, test-only loopback construction, and
move-only usage-settlement route.

That gate also decodes all four committed JSON manifests through closed
schemas, rejects unknown fields, machine-local paths and high-confidence secret
material, and cross-checks capability pins against the vendored manifests and
provenance records. The current pending Browse and Windows states are exact:
they cannot acquire measurements or advance to a qualified claim without a
reviewed validator change. This deliberately makes evidence promotion a
code-and-evidence review, rather than allowing an edited status string to turn
missing device work into a pass.

macOS CI also executes the hidden 14-case fixed-DOM matrix:

```sh
cargo run --locked -p zephium-engine \
  --features native-agentic-input-probe \
  --bin macos-agentic-input-probe -- --ci-hidden-fixed-dom
```

That mode never requests foreground focus or system-wide Accessibility access.
It is a deterministic safety regression test, not evidence for AppKit,
accessibility, focused OS input, a real site, or a production backend order.
Other matrices must run only on an explicitly authorized named device. Their
JSONL result remains in a local ignored location and is reviewed into aggregate
non-sensitive fields rather than committed raw.

The macOS production semantic adapter has a separate one-shot qualifier:

```sh
cargo run --locked -p zephium-engine \
  --features native-agentic-semantic-probe \
  --bin macos-agentic-semantic-probe -- --ci-hidden-fixed-dom
```

The reviewed 2026-09-03 result for this exact command is recorded in
`semantic-runtime-macos-v3.json`. It used one fixed 1280-by-800 logical hidden
owned view, one ephemeral `WKWebsiteDataStore`, three fixed loopback-only
documents, the ordinary content-policy installation seam, and the production
semantic registration. Five successful snapshots cover three isolated-world
epochs, one ref-bound fixed semantic click, and a host-released fixed
deferred-script mutation. The click changed the bound `Expanded` state from
false to true and the existing settlement/verifier core consumed the adjacent
snapshot as its declared postcondition. It produced an untrusted DOM event
without user activation, popup admission, or focus theft. An anchored request
for the removed node was refused as `AnchorMissing`,
and a subsequent fresh snapshot verified recovery without stale content. This
qualifies only the fixed semantic runtime and macOS adapter on the closed
fixture—not the full policy/host/controller path or arbitrary-site
compatibility. It performed no OS-wide input, requested no Accessibility
permission, opened no account or external site, and emitted only a content-free
aggregate. The source gate mechanically excludes its fixture, runner, and
feature from optimized and ordinary desktop builds. Re-running it still
requires separate explicit authorization; the committed result does not
authorize future native execution.

The release-excluded live-provider probe can run the reviewed public
Wikipedia workflow with GPT-5.6 Luna. It reads the development OpenAI key from
the fixed Login Keychain item through Apple's `/usr/bin/security` executable,
retains only these explicitly public qualification responses for dashboard
inspection, and emits content-free aggregate metrics locally:

```sh
cargo run --locked -p zephium-terra-macos-probe \
  --features live-probe -- --live-public-luna-suite-inspectable
```

The suite launches three unconstrained and three forced-locate workflows in
fresh AppKit processes. Each run fills Wikipedia's search control, obtains the
collapsed language option through the bounded semantic-locate path, selects
it, and independently verifies both native effects from fresh observations.
It is an external-network, paid-provider qualification and must be run only
with explicit authorization. Exact reviewed aggregate results are recorded in
`native-input-matrix-v1.json`; provider payloads, page content, credentials,
and raw traces must never be committed.

The Windows adapter can be compile-qualified from another host, but only a
physical Windows run is behavioral evidence. On an authorized named Windows
device, the preferred non-focused workflow is the checked-in fail-closed
orchestrator:

```powershell
.\scripts\qualification\windows-agentic.ps1 `
  -Phase CollectNonDebugger -AuthorizedPhysicalWindows
```

The acknowledgement is not authorization by itself; use it only after the
physical device and this exact run have been approved. The script refuses a
non-Windows or non-x86-64-MSVC host, a dirty or different checkout, a
reparse-point evidence directory, and any pre-existing directory entry. It
first runs the repository boundary gate, the complete offline probe-harness
tests, the production engine's agentic library tests, and the native
compile/link gates. Every Cargo invocation, including target-directory
discovery, runs in offline mode; dependencies must be fetched before the
authorized qualification begins. It then rechecks source cleanliness, revision
identity, and the still-empty result directory before recording the exact
revision in a create-new ignored source stamp. Only then does it capture the
four non-focused input modes and six non-debugger semantic modes, review the
input cohort, and rebuild the exact debugger target. It resolves Cargo's
active target directory, hashes the direct executable before collection,
requires the final build to
retain that digest, writes it into a second create-new stamp, and prints the
resolved path for the separately authorized manual debugger launch. Final
review rejoins the executable to that SHA-256 stamp before admitting the
debugger record. A failed preflight therefore cannot leave a stamp that
resembles collected evidence. The workflow has no focused-input invocation and
cannot launch the debugger-only mode.

The existing Windows CI job parses this PowerShell file through the native
PowerShell language parser without invoking either phase. That check is syntax
evidence only; the CI VM is not treated as a physical behavior run.

The individual transparent mode commands follow for review. They are not a
substitute for the source-bound orchestrator unless its full offline/native
preflight, post-preflight clean-revision check, create-new stamp, directory
inventory, debugger-binary hash binding, and source-continuity conditions are
independently enforced. Create the ignored local evidence directory and run
these exact closed modes in order:

```powershell
New-Item -ItemType Directory -Force `
  eval/agentic-browsing/local-results | Out-Null
cargo --offline check --locked --target x86_64-pc-windows-msvc `
  -p zephium-engine --features agentic-browser
cargo --offline run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --ci-hidden-fixed-dom `
  --evidence-directory eval/agentic-browsing/local-results
cargo --offline run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --ci-hidden-hwnd `
  --evidence-directory eval/agentic-browsing/local-results
cargo --offline run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --ci-hidden-cdp `
  --evidence-directory eval/agentic-browsing/local-results
cargo --offline run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --visible-background-windows-all `
  --evidence-directory eval/agentic-browsing/local-results
cargo --offline run --locked -p zephium-agentic `
  --features probe-harness `
  --bin windows-agentic-input-evidence-review -- `
  --directory eval/agentic-browsing/local-results `
  --write-summary
```

These modes use only an ephemeral user-data directory, InPrivate controller,
loopback fixtures, one owned probe window, and fixed commands. They do not use
OS-wide input, accounts, credentials, external sites, or extension-owned
native seams. The visible-focused mode is separately gated by the literal
`--allow-visible-focused` argument and must not be run without explicit
foreground authorization. Construction and each row re-attest the exact native
host/container/controller/document ownership, visibility, and DPI-rounded
viewport. Hidden/background qualification samples foreground, active-window,
caller-queue focus, and exact document-thread active/focus state before,
during, and after dispatch. Each fixed input submission also has adjacent
pre/post samples, so a transient transfer observed at those boundaries fails
even if it reverses before fixture settlement. The document-thread projection
uses read-only `GetGUIThreadInfo`; the runner never attaches input queues.

Each runner process writes exactly one versioned, size-bounded `ProbeResponse`
JSON record to its selected machine-evidence sink before applying its pass/fail
qualification whenever the native matrix returns evidence or a typed
rejection. Build progress and the aggregate pass/fail summary remain on stderr,
so a nonzero exit may still leave the redacted evidence needed to diagnose a
rejected backend. The ignored local records contain only the closed evidence
schema; review must still reject any
unexpected file before extracting aggregate fields into committed evidence.
For the required physical workflow, `--evidence-directory` is accepted only
with the literal ignored directory and four required non-focused modes. It
preflights a missing fixed destination before native work, writes the validated
UTF-8 bytes to a temporary file in that directory, syncs them, and atomically
publishes without replacing any existing record. This avoids PowerShell native
redirection encoding differences. Archive a prior result directory before a
rerun; neither runner nor reviewer overwrites evidence. The stdout form remains
available for an individually authorized diagnostic run.

The final review command is offline and can run on the same Windows device or
another trusted checkout. It opens only the four fixed filenames as direct
regular files under one direct directory; on Windows, both runners and both
offline reviewers additionally reject the `FILE_ATTRIBUTE_REPARSE_POINT` bit,
so a junction, mount point, symbolic link, or other reparse provider cannot be
treated as a direct evidence path. It decodes exactly one bounded JSONL
response from each; requires the response/run identity, Windows/WebView2
runtime, adapter revision, capability inventory, case/backend order,
presentation, focus, resource, outcome, trust, activation, and teardown joins;
and requires one identical runtime fingerprint across all four processes. It
never emits the captured records or file paths. Without `--write-summary`, its
sole stdout record is a bounded content-free aggregate suitable for human
review; a typed rejection, partial matrix, runtime substitution, focus theft,
or failed fixture makes the review command nonzero. The exact
`--write-summary` form instead writes those same validated UTF-8 aggregate
bytes to the fixed create-new `windows-review-summary-v2.json` filename. Review
schema v2 includes maximum derived helper-process count and aggregate
Environment8-cohort resident working-set bytes; individual process identities
remain absent. WebView2 excludes crashpad from that API, so this is not a
whole-process-family measurement. The aggregate
remains ignored until reviewed into the committed manifest.

The production Windows semantic adapter has a separate seven-process physical
qualification. It is fully hidden and uses no OS-wide pointer or keyboard
input, focus API, Accessibility permission, account, credential, external
site, extension-owned native seam, screenshot, or page-world bridge. Each run
creates a new ephemeral WebView2 user-data directory, an InPrivate automation
subprofile whose extension inventory is proven empty, one fixed 1280-by-800
logical owned view, and only loopback fixtures. The release-excluded renderer
case issues the sole fixed `Page.crash` diagnostic command against that local
view; no generic CDP method, expression, selector, or target is accepted.
Every accepted document also consumes the production `SourceChanged`/
`HistoryChanged` claim by sampling WebView2's bounded native `Source`, parsing
the canonical target, and requiring it to equal the committed URL. A pending,
dirty, substituted, or teardown-surviving location claim fails the run.
Every mode also takes a stable, bounded WebView2 Environment8 process-cohort
and aggregate resident-working-set sample before its mode-specific work and
after semantic work drains. Qualification requires exactly one Browser kind
and at least one helper, admits at most 64 unique nonzero process IDs, opens
only non-inheritable query-limited handles, checked-sums under 1 TiB, and
exactly rejoins the PID/kind, total-count, and helper-count cohort while those
handles remain owned. `GetProcessInfos` excludes crashpad, so the aggregate is
not a whole-process-family or resource-budget measurement. Missing, partial,
changed, overflowing, or close-failed samples reject the run; process
identities never enter evidence.

Run the six non-debugger modes as ordinary processes on an authorized
physical Windows device:

```powershell
cargo --offline run --locked -p zephium-engine `
  --features native-agentic-semantic-probe `
  --bin windows-agentic-semantic-probe -- --ci-hidden-fixed-documents `
  --evidence-directory eval/agentic-browsing/local-results
cargo --offline run --locked -p zephium-engine `
  --features native-agentic-semantic-probe `
  --bin windows-agentic-semantic-probe -- --ci-hidden-redirect-lifecycle `
  --evidence-directory eval/agentic-browsing/local-results
cargo --offline run --locked -p zephium-engine `
  --features native-agentic-semantic-probe `
  --bin windows-agentic-semantic-probe -- --ci-hidden-location-replacement `
  --evidence-directory eval/agentic-browsing/local-results
cargo --offline run --locked -p zephium-engine `
  --features native-agentic-semantic-probe `
  --bin windows-agentic-semantic-probe -- --ci-hidden-suspend-resume `
  --evidence-directory eval/agentic-browsing/local-results
cargo --offline run --locked -p zephium-engine `
  --features native-agentic-semantic-probe `
  --bin windows-agentic-semantic-probe -- --ci-hidden-event-flood `
  --evidence-directory eval/agentic-browsing/local-results
cargo --offline run --locked -p zephium-engine `
  --features native-agentic-semantic-probe `
  --bin windows-agentic-semantic-probe -- --ci-hidden-renderer-loss `
  --evidence-directory eval/agentic-browsing/local-results
cargo --offline build --locked -p zephium-engine `
  --features native-agentic-semantic-probe `
  --bin windows-agentic-semantic-probe
```

For the seventh mode, configure an authorized debugger to launch the exact
freshly built executable path printed by the preferred orchestrator. That path
comes from Cargo metadata rather than an assumption about `target-dir` (the
usual default is `target\debug\windows-agentic-semantic-probe.exe`). Use these
exact arguments and keep the debugger attached until process exit:

```text
--ci-hidden-debugger-coexistence --evidence-directory eval/agentic-browsing/local-results
```

After that process exits, the preferred final review is:

```powershell
.\scripts\qualification\windows-agentic.ps1 -Phase ReviewAfterDebugger
```

This phase refuses source-revision drift, a changed/missing/reparse debugger
binary, a noncanonical or mismatched create-new SHA-256 stamp, missing or
reparse records, and every directory entry other than the two exact stamps,
four input records, input summary, and seven semantic records before creating
the semantic summary.

The debugger mode fails before native construction if no debugger is attached;
all other modes fail before construction if one is attached. The suspension
mode runs the production `TrySuspend` adapter, verifies the final suspended
bit, resumes with final active readback, and requires a fresh same-document
semantic observation. The redirect mode first requires the production gate to
refuse the fixed loop at exactly eight admitted redirect events, then proves
recovery by committing the fixed two-hop chain to its authoritative final URL
and taking a fresh semantic snapshot. The location mode reaches native load
completion before a one-shot loopback script response is held, captures the
pre-mutation semantics, releases one fixed `history.replaceState`, and then
requires WebView2's native current `Source`, the functional-core successor,
stale-prior refusal, native rejoin, and a fresh post-mutation snapshot. Every mode also
samples debugger state and foreground, active-window, and thread-focus state
throughout native work. Review only the exact seven create-new records:

```powershell
cargo --offline run --locked -p zephium-agentic `
  --features probe-harness `
  --bin windows-agentic-semantic-evidence-review -- `
  --directory eval/agentic-browsing/local-results `
  --write-summary
```

The offline reviewer requires one identical Windows/WebView2/adapter
fingerprint. Protocol v5 and the
`semantic-runtime-m3-lifecycle-m2-redirect-location-resources-v4` adapter revision
prevent older six-record results from mixing with this cohort. It
also requires exact mode-specific snapshot, document-epoch, suspension,
pressure, bounded redirect/refusal/recovery, same-document native replacement/
rejoin/stale-prior recovery, renderer-loss, debugger, focus, pending-work,
process-exit, profile, fixture, native-view teardown, and before/after resource
facts. Review schema v5 retains only the maximum process count and aggregate
resident bytes alongside the bounded suspend-callback duration; it retains no
process identity, page content, path, native error, world/context identity, or
trace. It reads no other filename. The runner
accepts no stdout form and neither runner nor reviewer overwrites an existing
record. It shares the same direct-path/reparse-point rejection as the input
evidence path. Archive the local directory before any retry.

The authorized hidden fixed-DOM macOS safety result and the bounded allowlisted
Wikipedia fill slice are recorded only as reviewed aggregate fields. Native
AppKit/accessibility, visible/background behavior, physical Windows, the
difficult-site matrix, and Browse baseline results remain empty until their
separately authorized runs occur. `pending_device_capture` is a blocking
evidence state, not a passing result or an inferred zero.
