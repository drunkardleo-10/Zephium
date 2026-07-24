# Blocker fuzz harness

This is an independent, non-published `cargo-fuzz` workspace. Its targets use
only bounded, Zephium-authored inputs under the reserved `.invalid` namespace.
They perform no network or filesystem I/O.

The retained seeds are regression starting points, not evidence of a completed
fuzz campaign. Run with the repository's reviewed nightly and pinned
`cargo-fuzz`:

```text
cd crates/zephium-blocker/fuzz
cargo +nightly-2026-07-20 fuzz run source_admission -- -max_len=131072
cargo +nightly-2026-07-20 fuzz run request_match -- -max_len=16384
cargo +nightly-2026-07-20 fuzz run webkit_canonical -- -max_len=131072
```

Preserve minimized regressions in the corresponding `corpus/` directory only
after confirming they contain no third-party filter-list material, page URLs,
credentials, or browsing data. Long-running sanitizer campaigns and native
adapter testing remain release evidence outside this harness.
