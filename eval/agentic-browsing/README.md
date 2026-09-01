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
cargo check --locked --target x86_64-pc-windows-msvc \
  -p zephium-engine --features agentic-browser
cargo check --locked --target x86_64-pc-windows-msvc \
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

The reviewed 2026-09-01 result for this exact command is recorded in
`semantic-runtime-macos-v1.json`. It used one fixed 1280-by-800 logical hidden
owned view, one ephemeral `WKWebsiteDataStore`, two fixed loopback-only
documents, the ordinary content-policy installation seam, and the production
semantic registration. It performed no OS-wide input, requested no
Accessibility permission, opened no account or external site, and emitted only
a content-free aggregate. The source gate mechanically excludes its fixture,
runner, and feature from optimized and ordinary desktop builds. Re-running it
still requires separate explicit authorization; the committed result does not
authorize future native execution.

The Windows adapter can be compile-qualified from another host, but only a
physical Windows run is behavioral evidence. On an authorized named Windows
device, create the ignored local evidence directory and run these exact closed
modes in order:

```powershell
New-Item -ItemType Directory -Force `
  eval/agentic-browsing/local-results | Out-Null
cargo check --locked --target x86_64-pc-windows-msvc `
  -p zephium-engine --features agentic-browser
cargo run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --ci-hidden-fixed-dom `
  --evidence-directory eval/agentic-browsing/local-results
cargo run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --ci-hidden-hwnd `
  --evidence-directory eval/agentic-browsing/local-results
cargo run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --ci-hidden-cdp `
  --evidence-directory eval/agentic-browsing/local-results
cargo run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --visible-background-windows-all `
  --evidence-directory eval/agentic-browsing/local-results
cargo run --locked -p zephium-agentic `
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
and thread-focus state before, during, and after dispatch so a transient focus
transfer fails even if it reverses before fixture settlement.

Each runner process writes exactly one versioned, size-bounded `ProbeResponse`
JSON record to its selected machine-evidence sink before applying its pass/fail
qualification whenever the native matrix returns evidence or a typed
rejection. Build progress and the aggregate pass/fail summary remain on stderr,
so a nonzero exit may still leave the redacted evidence needed to diagnose a
rejected backend. The ignored local
records contain only the closed evidence schema; review must still reject any
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
another trusted checkout. It opens only the four fixed filenames as real,
non-symlink files under one real directory; decodes exactly one bounded JSONL
response from each; requires the response/run identity, Windows/WebView2
runtime, adapter revision, capability inventory, case/backend order,
presentation, focus, resource, outcome, trust, activation, and teardown joins;
and requires one identical runtime fingerprint across all four processes. It
never emits the captured records or file paths. Without `--write-summary`, its
sole stdout record is a bounded content-free aggregate suitable for human
review; a typed rejection, partial matrix, runtime substitution, focus theft,
or failed fixture makes the review command nonzero. The exact
`--write-summary` form instead writes those same validated UTF-8 aggregate
bytes to the fixed create-new summary filename. The aggregate remains ignored
until reviewed into the committed manifest.

The production Windows semantic adapter has a separate four-process physical
qualification. It is fully hidden and uses no OS-wide pointer or keyboard
input, focus API, Accessibility permission, account, credential, external
site, extension-owned native seam, screenshot, or page-world bridge. Each run
creates a new ephemeral WebView2 user-data directory, an InPrivate automation
subprofile whose extension inventory is proven empty, one fixed 1280-by-800
logical owned view, and only loopback fixtures. The release-excluded renderer
case issues the sole fixed `Page.crash` diagnostic command against that local
view; no generic CDP method, expression, selector, or target is accepted.

Run the three non-debugger modes as ordinary processes on an authorized
physical Windows device:

```powershell
cargo run --locked -p zephium-engine `
  --features native-agentic-semantic-probe `
  --bin windows-agentic-semantic-probe -- --ci-hidden-fixed-documents `
  --evidence-directory eval/agentic-browsing/local-results
cargo run --locked -p zephium-engine `
  --features native-agentic-semantic-probe `
  --bin windows-agentic-semantic-probe -- --ci-hidden-event-flood `
  --evidence-directory eval/agentic-browsing/local-results
cargo run --locked -p zephium-engine `
  --features native-agentic-semantic-probe `
  --bin windows-agentic-semantic-probe -- --ci-hidden-renderer-loss `
  --evidence-directory eval/agentic-browsing/local-results
cargo build --locked -p zephium-engine `
  --features native-agentic-semantic-probe `
  --bin windows-agentic-semantic-probe
```

For the fourth mode, configure an authorized debugger to launch the exact
freshly built executable
`target\debug\windows-agentic-semantic-probe.exe` with these exact arguments,
and keep it attached until process exit:

```text
--ci-hidden-debugger-coexistence --evidence-directory eval/agentic-browsing/local-results
```

The debugger mode fails before native construction if no debugger is attached;
all other modes fail before construction if one is attached. Every mode also
samples debugger state and foreground, active-window, and thread-focus state
throughout native work. Review only the exact four create-new records:

```powershell
cargo run --locked -p zephium-agentic `
  --features probe-harness `
  --bin windows-agentic-semantic-evidence-review -- `
  --directory eval/agentic-browsing/local-results `
  --write-summary
```

The offline reviewer requires one identical Windows/WebView2/adapter
fingerprint and exact mode-specific snapshot, document-epoch, pressure,
recovery, renderer-loss, debugger, focus, pending-work, process-exit, profile,
fixture, and native-view teardown facts. It reads no other filename and emits
no page content, path, native error, world/context identity, or trace. The
runner accepts no stdout form and neither runner nor reviewer overwrites an
existing record. Archive the local directory before any retry.

The authorized hidden fixed-DOM macOS safety result is recorded only as reviewed
aggregate fields. Native AppKit/accessibility, visible/background behavior,
physical Windows, real-site, and Browse baseline results remain empty until
their separately authorized runs occur. `pending_device_capture` is a blocking
evidence state, not a passing result or an inferred zero.
