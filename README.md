# Zephium

Zephium is a FOSS browser built in Rust on the operating system's native web
engine: WKWebView on macOS, WebView2 on Windows, and WebKitGTK on Linux.

The project is designed around a lightweight native bundle, fast startup, low
resource use, local-first data, and a workspace-oriented browsing experience.
The current development phase focuses on the backend and native foundation:
security boundaries, profile isolation, lifecycle correctness, persistence,
resource management, and cross-platform WebView integration.

## Architecture

Untrusted sites run in raw Wry WebViews, separate from the privileged Tauri
views used by the application chrome. Zephium maintains narrow in-tree Tauri,
Tauri Runtime Wry, and Wry adapters for storage policy, transactional native
construction, WebView callbacks, lifecycle, and teardown.

Profiles partition native website data and Zephium-owned history, session, and
favicon data. Platform runtime admission keeps the browser on reviewed WebKit,
WebView2, and WebKitGTK security releases.

Engineering references:

- [Architecture](docs/architecture.md)
- [Security model](docs/security-model.md)
- [Security maintenance](docs/security-maintenance.md)
- [Frontend/native boundary](docs/frontend.md)
- [Native network blocker](docs/adblock.md)
- [Release engineering](.github/RELEASE.md)

## Development

Install the current Rust toolchain, pnpm, and the platform WebView development
packages. Linux additionally needs the GTK3 and WebKitGTK 4.1 development
stack used by CI.

```sh
pnpm install --frozen-lockfile
pnpm dev
```

The native runtime security floor is enforced in development. If startup exits
with status 78 because the installed OS or WebView runtime is below the named
floor, update the platform runtime and start Zephium again.

Useful validation commands:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --release --locked
cargo xtask check-engine-floors
pnpm --dir frame check
pnpm --dir frame build
```

Each native-boundary fork has its own lockfile and standalone validation. The
complete local gate is `cargo xtask ci`; individual adapters can also be run
directly:

```sh
cargo test --manifest-path vendor/tauri/Cargo.toml --locked --all-targets --features macos-private-api,specta
cargo test --manifest-path vendor/tauri-runtime-wry/Cargo.toml --locked --all-targets --features macos-private-api
cargo test --manifest-path vendor/wry/Cargo.toml --locked --all-targets
cargo clippy --manifest-path vendor/tauri/Cargo.toml --locked --all-targets --features macos-private-api,specta -- -D warnings
cargo clippy --manifest-path vendor/tauri-runtime-wry/Cargo.toml --locked --all-targets --features macos-private-api -- -D warnings
cargo clippy --manifest-path vendor/wry/Cargo.toml --locked --all-targets -- -D warnings
```

## License

Zephium is licensed under MPL-2.0. Vendored dependencies retain their upstream
licenses and provenance. The bundled EasyList/EasyPrivacy snapshots retain
their separately documented CC BY-SA 3.0 terms and attribution in
[assets/blocker-seed/v1](assets/blocker-seed/v1). See also the
[adblock-rust](vendor/adblock/UPSTREAM.md),
[Tauri](vendor/tauri/UPSTREAM.md),
[Tauri Runtime Wry](vendor/tauri-runtime-wry/UPSTREAM.md), and
[Wry](vendor/wry/UPSTREAM.md) fork records.
