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
cargo test --locked -p zephium-engine \
  --features native-agentic-input-probe --lib
cargo test --locked -p zephium-engine \
  --features native-agentic-input-probe \
  --bin macos-agentic-input-probe
cargo clippy --locked -p zephium-engine \
  --features native-agentic-input-probe \
  --bin macos-agentic-input-probe --lib -- -D warnings
cargo check --locked --target x86_64-pc-windows-msvc \
  -p zephium-engine --features native-agentic-input-probe \
  --bin windows-agentic-input-probe
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
`probe-harness` or `native-agentic-input-probe`. All probe-only contract,
control, evidence, protocol, recipe, fixture, and native modules remain behind
those features.

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

The Windows adapter can be compile-qualified from another host, but only a
physical Windows run is behavioral evidence. On an authorized named Windows
device, run these exact closed modes in order:

```powershell
cargo run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --ci-hidden-fixed-dom
cargo run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --ci-hidden-hwnd
cargo run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --ci-hidden-cdp
cargo run --locked -p zephium-engine `
  --features native-agentic-input-probe `
  --bin windows-agentic-input-probe -- --visible-background-windows-all
```

These modes use only an ephemeral user-data directory, InPrivate controller,
loopback fixtures, one owned probe window, and fixed commands. They do not use
OS-wide input, accounts, credentials, external sites, or extension-owned
native seams. The visible-focused mode is separately gated by the literal
`--allow-visible-focused` argument and must not be run without explicit
foreground authorization.

The authorized hidden fixed-DOM macOS safety result is recorded only as reviewed
aggregate fields. Native AppKit/accessibility, visible/background behavior,
physical Windows, real-site, and Browse baseline results remain empty until
their separately authorized runs occur. `pending_device_capture` is a blocking
evidence state, not a passing result or an inferred zero.
