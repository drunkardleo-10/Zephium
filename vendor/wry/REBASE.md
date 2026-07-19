# Wry fork update procedure

Zephium treats this fork as a native security adapter, not as a source copy
that can be refreshed with a version bump. Every update must account for the
complete upstream range and every patch set in `FORK.toml`.

## 1. Freeze and record the inputs

Work from a dedicated branch based on an immutable, fully reviewed Zephium
commit. The working tree must be clean. Record:

- the current `FORK.toml` upstream commit and standalone lockfile digest;
- the proposed upstream tag and full commit SHA;
- the upstream release notes, security notices, and dependency changes;
- the reviewer and date for each native platform.

Fetch upstream from `https://github.com/tauri-apps/wry` into a separate review
checkout. Verify the tag-to-commit relationship and obtain the source from that
exact commit. Do not use a moving branch, an unverified archive, or a package
registry version as the source of truth. If the upstream tag or release assets
cannot be authenticated, stop the update.

## 2. Preserve a complete fork delta

Before replacing any source, produce and retain a recursive, binary-capable
diff between:

1. a clean checkout of the old upstream commit from `FORK.toml`; and
2. the current `vendor/wry` tree, excluding only the explicitly Zephium-owned
   provenance documents and generated standalone lockfile.

Also produce the complete old-upstream-to-new-upstream diff with rename
detection disabled. Native security review must see additions and deletions as
content changes; a rename must not hide a callback, delegate, feature default,
unsafe block, dependency, build script, or platform-specific implementation.
Retain both diffs with the pull-request evidence.

Inventory every changed upstream path against `FORK.toml`. An upstream change
outside the existing patch-set paths is still in scope when it affects public
builder defaults, features, dependencies, platform selection, construction,
navigation, evaluation, protocol handling, or destruction.

## 3. Decide every local patch explicitly

Create a review table with one row per `[[patch_sets]]` entry and one of these
outcomes:

- **reapplied** — the invariant is still local and has tests on the new base;
- **reworked** — upstream changed the seam, with the replacement design and
  tests linked;
- **retired upstream** — upstream now enforces the same or stronger invariant,
  with exact source and tests cited;
- **removed intentionally** — the application no longer uses the capability,
  with all call sites and documentation removed.

“Applied cleanly” is not a security conclusion. Re-review page-reachable
callbacks, optional native values, exactly-once completion, teardown, and
thread/apartment ownership even when the textual patch has no conflict.

The following boundaries always require line review on all supported
platforms:

- raw-page IPC, custom protocols, script evaluation, host objects, and injected
  scripts;
- navigation identities, redirects, popup/new-window handling, and the
  commit-to-presentation/input barrier;
- permission, authentication, certificate, file, print, media, fullscreen,
  preview, dialog, download, and drag/drop surfaces;
- per-profile persistent/ephemeral stores, cookies, caches, and process models;
- WebView2 COM apartment ownership, deferrals, registrations, staged
  construction, controller close, child HWND destruction, and process-exit
  proof;
- WKWebView retained ownership, Objective-C superclass/ivar layout and
  alignment, delegates, navigation-object lifetime, data stores, and
  main-thread dispatch;
- WebKitGTK context ownership, signal lifetimes, async completion, sandbox
  configuration, renderer confinement, and GTK widget destruction;
- every `unsafe` block, `unwrap`/`expect`, panic path, unbounded registry,
  thread spawn, and process-global mutable state reachable from native or page
  callbacks.

## 4. Import and rebuild deliberately

Start from the exact new upstream source, then reapply the reviewed adapter
changes. Do not copy new upstream files selectively over the old tree. Keep
Zephium provenance files and update them only after the implementation review
is complete.

Reconcile public API and every Zephium call site before regenerating locks.
Dependency changes require an explicit source/license/advisory review. Update
both dependency graphs deliberately:

```sh
cargo update --manifest-path vendor/wry/Cargo.toml
cargo update --workspace
```

Review the complete diff of both `vendor/wry/Cargo.lock` and the repository
`Cargo.lock`. Never delete a lockfile to make resolution succeed. No unreviewed
Git dependency, alternate registry, or source replacement may enter either
graph.

Finally update `FORK.toml` and `UPSTREAM.md` with the exact version, commit,
review date, patch inventory, and any changed limitations. A patch set may be
removed from `FORK.toml` only when its review-table disposition is recorded.

## 5. Validate the adapter independently

The fork is excluded from the application workspace, so run its standalone
matrix explicitly on Windows, macOS, and the supported Fedora image:

```sh
cargo fmt --manifest-path vendor/wry/Cargo.toml -- --check
cargo check --manifest-path vendor/wry/Cargo.toml --locked --all-targets
cargo test --manifest-path vendor/wry/Cargo.toml --locked --all-targets
cargo clippy --manifest-path vendor/wry/Cargo.toml --locked --all-targets -- -D warnings
cargo check --manifest-path vendor/wry/Cargo.toml --locked --release
```

On the supported Fedora image, also run the ignored native presentation gate
under Xvfb:

```sh
xvfb-run -a cargo test --manifest-path vendor/wry/Cargo.toml --locked \
  --lib \
  web_context::tests::guarded_webkitgtk_construction_commit_and_first_map_are_native_and_fail_closed \
  -- --ignored --exact
```

Then run the locked workspace check/test/Clippy/release matrix. Add fault
injection at every new or moved native construction, registration, callback,
dispatch, completion, teardown, and destruction boundary. Exercise malformed
optional values, callback reentrancy, queue saturation, cancellation,
navigation overlap, renderer/browser crashes, and failures after partial native
ownership is acquired.

Source-level tests are not release evidence. Install the packaged candidate on
real supported Windows, macOS, and Fedora hosts and run the hostile-page,
prompt-denial, redirect/anti-spoof, profile isolation/erasure, sandbox/process
mitigation, crash recovery, split/resize, and shutdown suites. Repeat the
1/10/50/100-tab and endurance measurements because an upstream engine adapter
change can alter process count, wakeups, and lifetime even when APIs compile.

## 6. Review, land, and recover

At least two maintainers review the complete fork update; at least one review
must focus on native ownership and page-reachable policy rather than API
compatibility. A stable release additionally requires the independent
native-boundary audit and packaged gates named in `FORK.toml`.

Land the update as one reviewable Conventional Commit that names the upstream
version and immutable commit in its body. Attach the patch-disposition table
and validation evidence to the pull request. Do not land an intermediate tree
that silently uses vanilla Wry defaults or cannot pass the locked build.

If any invariant, native cleanup obligation, platform test, dependency source,
or provenance check is ambiguous, stop and keep the previous reviewed fork.
Rollback means restoring the complete previous Zephium commit and both
lockfiles; never mix old adapter source with a new dependency graph. A release
already signed from an invalid update is a security incident and follows the
revocation procedure in `.github/RELEASE.md`.
