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
cargo xtask check-agentic-probe-boundary
```

An optimized build with `probe-harness` is expected to fail at compile time.
The ordinary `zephium-desktop` resolved graph is separately required not to
reach `zephium-agentic`.

Named-device results remain empty until the native probe is run on authorized
macOS and Windows hosts. `pending_device_capture` is a blocking evidence state,
not a passing result or an inferred zero.
