# Security maintenance runbook

Zephium deliberately treats the operating-system WebView runtime and the
application build graph as security-sensitive release inputs. The checks
described here fail closed. A red check is an instruction to review evidence,
not a deadline or advisory string to extend mechanically.

## Ownership and cadence

The release security owner reviews all three native-engine channels at least
weekly and again immediately before freezing a release commit. A second
maintainer reviews every floor change used for a signed artifact. The review
must happen sooner when Apple, Microsoft, WebKitGTK, RustSec, npm, a supported
distribution, or the Tauri or Wry projects publish a security notice.

Current deadlines are encoded next to their evidence in:

- `crates/zephium-core/src/macos.rs`
- `crates/zephium-core/src/webview2.rs`
- `crates/zephium-core/src/webkitgtk.rs`
- `deny.toml` for temporary advisory exceptions

CI enforces these dates with:

```sh
cargo xtask check-engine-floors
cargo xtask check-advisory-exceptions
```

The production workflow uses the stricter release-only gate:

```sh
cargo xtask check-release-engine-security
```

It also fails when a vendor has publicly acknowledged a browser-engine fix
that is not yet available in its supported Stable runtime. Development may
continue on the newest admitted runtime, but an unavailable vendor patch is
never treated as production release evidence.

Calendar alerts should be set for seven days before every encoded deadline.
The alerts are operational backup only; CI remains the authoritative
fail-closed control.

## Native-engine floor review

For each platform:

1. Read the vendor's current security advisory and release-note source, not a
   search result or version aggregator.
2. Identify every vendor-supported operating-system or stable runtime line
   Zephium claims to support. Do not admit preview, development, or newly
   numbered release lines by numeric comparison alone.
3. Record the minimum fixed version, publication date, immutable source URL,
   newest reviewed version, and next review deadline together.
4. Confirm startup admission, CI packages, packaged-artifact checks, and the
   documentation all describe the same boundary.
5. Add or update boundary tests for the exact version below the floor, the
   floor itself, the latest reviewed version, an unknown release line, a clock
   before publication, and the first instant after review expiry.
6. Run the platform's packaged hostile/native suite. A source-level version
   check is not renderer-confinement or process-mitigation evidence.
7. Preserve links, command output, package metadata, and test artifacts with
   the reviewed release commit.

If the vendor has disclosed fixes but has not yet published a supported stable
runtime, the release remains blocked. Do not extend the review date merely to
make CI green. Development may continue on a documented non-release branch,
but no signed production artifact may bypass the gate.

## Dependency advisories

Rust dependency policy is enforced with the locked graph and `cargo deny`.
An exception in `deny.toml` requires an owner, reason, removal plan, and expiry
within the bounded review horizon. On expiry, remove or upgrade the dependency;
extending an exception is a new security decision and requires review.

The complete pnpm graph is intentionally checked at `high` severity:

```sh
pnpm audit --audit-level high
```

This includes development dependencies because Vite, package plugins, and
other build tools execute while producing signed artifacts. When this check
fails:

1. Confirm the advisory and affected resolved version against the package
   registry/advisory source.
2. Determine whether the dependency executes in development, CI, release
   production, or the shipped application.
3. Upgrade or remove it and regenerate the lockfile deliberately.
4. Re-run frontend checks, the production build, and the release-script tests.
5. If no fixed dependency graph exists, keep production release blocked. Do
   not lower the audit severity or omit development dependencies as a release
   workaround.

Registry availability is different from a vulnerability result. A network or
registry failure should be retried from the controlled release environment;
it must not be converted into a passing audit.

## Native-boundary fork maintenance

The in-tree Tauri, Tauri Runtime Wry, and Wry forks are narrow native security
adapters and therefore part of Zephium's trusted computing base. Monitor
upstream security releases and the files touched by every upstream change.
Follow the `REBASE.md` in each fork for every update; a version-only bump or
blind merge is prohibited. Tauri and Tauri Runtime Wry share an upstream
repository and commit, but their complete deltas, lockfiles, and validation
results remain independently reviewable.

Before a stable release, all native-platform checks must run on the immutable
candidate commit. Passing macOS tests does not substitute for Windows or Linux,
and unit tests do not substitute for packaged hostile-page, teardown, erasure,
and endurance tests.

The Linux Wayland device pass must also force GlobalShortcuts portal denial
(and separately restart the portal process) and verify that the configured
launcher chord still opens from the focused main window and closes from the
focused launcher panel. After permission is restored, verify one activation per
press, immediate fallback after an authoritative `ShortcutsChanged` removal,
and no activation after browser shutdown begins. Run this against the packaged
artifact whose installed entry is exactly `app.zephium.desktop`; raw `cargo`
or `tauri dev` execution does not prove portal application identity.

A teardown-only GLib-GIO warning from a sandbox child saying that release of an
`app.zephium.Sandboxed.WebProcess-*` bus name failed because its connection was
already closed is a WebKit child-process cleanup diagnostic. Do not hide it
with a GLib log handler or by weakening the WebKit sandbox. It is acceptable
only when Zephium exits cleanly and it is not accompanied by an
`engine: web process terminated` event, a core dump, a hang, or a surviving
auxiliary process. Any of those accompanying symptoms turns it into a native
runtime failure: retain the journal and core, record the exact WebKitGTK/GLib
versions, and keep the candidate blocked pending engine-level investigation.

## Release evidence

For every candidate, retain:

- the exact commit and annotated tag;
- native runtime and OS versions for every runner/device;
- engine-floor and advisory review output;
- locked dependency manifests and SBOMs;
- packaged hostile/native and profile-erasure results;
- resource/endurance measurements;
- signing, notarization, provenance, and installer verification output.

Any source, lockfile, floor, exception, build image, signing input, or workflow
change invalidates the prior evidence and requires a new candidate run.
