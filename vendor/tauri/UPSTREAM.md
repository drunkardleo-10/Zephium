# Vendored Tauri provenance

This directory is the `tauri` crate at version 2.11.3 from the immutable
upstream revision
`6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a`:

<https://github.com/tauri-apps/tauri/commit/6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a>

It is a narrow policy fork. Upstream unconditionally supplies a LocalData
directory to Linux WebViews that omit one. That turns an incognito WebView's
otherwise pathless context into a persistent context before Wry can enforce
ephemeral storage. Zephium keeps the directory absent for implicit Linux
incognito construction. Normal Linux views retain upstream's synthesized
directory, and Windows behavior is unchanged. Supplying both incognito mode
and an explicit Linux directory is a typed construction error raised before
the directory can be created.

The policy matrix is implemented as a pure decision function so every Linux
and Windows case runs on every CI host. `FORK.toml` records the complete local
delta; [`REBASE.md`](REBASE.md) is mandatory for upstream updates.

When `vendor/tauri/Cargo.toml` is used as the root manifest, its
`[patch.crates-io]` table selects Zephium's sibling `tauri-runtime-wry` and Wry
forks. Cargo ignores dependency-level patch tables in application builds; the
repository root supplies the same paths there. `Cargo.toml.orig` remains the
unaltered crates.io manifest for provenance comparison.

The published normalized manifest also omits upstream
`Cargo.toml.orig`'s self dev-dependency. Standalone unit tests expand
`generate_context!` code that resolves the `tauri` crate alias, so
`Cargo.toml` restores that exact path dependency with default features off and
`wry` enabled. It affects only this fork's test graph.

Zephium's production graph exact-pins Specta 2.0.0-rc.25. The fork applies the
same pin to Tauri's optional Specta integration, makes its production-required
`std` support explicit instead of relying on workspace feature unification,
and uses its current `Types` registry API. This deliberately narrows upstream's
pre-release range: the standalone adapter must validate the API, features, and
dependency revision used by the application, rather than an older independently
resolved release candidate.

The published crate excludes `/test` even though two upstream library test
modules compile-time include its normal application fixture. The five required
files are restored byte-for-byte from the same immutable commit solely for
standalone unit tests. [`TEST_FIXTURE.toml`](TEST_FIXTURE.toml) records each Git
blob identity; these files are not linked into Zephium's production binary.
