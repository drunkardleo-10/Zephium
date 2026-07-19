# Tauri fork update procedure

Zephium maintains this as a narrow policy adapter. An update must not replace
it with an unreviewed crates.io source.

1. Freeze a clean Zephium commit and record the old revision and lockfiles.
2. Fetch the proposed Tauri tag and immutable commit from
   `https://github.com/tauri-apps/tauri`. Review the complete old-to-new diff,
   including `crates/tauri`, `tauri-runtime-wry`, and relevant Wry changes.
3. Compare clean upstream `crates/tauri` at the old revision with this entire
   directory, excluding only Zephium provenance files and the standalone
   lockfile. Explicitly classify every `FORK.toml` patch as reapplied,
   reworked, retired upstream, or intentionally removed.
4. Re-review data-directory resolution and creation in
   `src/manager/webview.rs`. Prove all eight Linux/Windows combinations of
   incognito state and caller-supplied directory still match the tests.
5. Import the complete new upstream crate, reapply the reviewed policy, and
   update the version, commit, lockfiles, and provenance together. Never mix
   source from different upstream revisions. Preserve the standalone-only
   sibling patches in `Cargo.toml`; keep `Cargo.toml.orig` as upstream shipped
   it and regenerate `Cargo.lock` against the reviewed sibling forks. Keep the
   optional Specta pin aligned with the application's exact reviewed version,
   and re-review its `FunctionArg` registry API before changing the pin. If the
   published crate still omits fixtures referenced by its unit tests, import
   only the required files from the same commit and verify every Git blob
   against `TEST_FIXTURE.toml` before running tests.
6. Run standalone formatting plus the locked workspace check, tests, Clippy,
   and release check. Run packaged startup, profile-isolation, persistence,
   incognito, and erasure tests on supported Windows, macOS, and Fedora hosts.
7. Require two reviewers, including one focused on native storage and
   construction policy. If an invariant cannot be proved, keep the previous
   reviewed fork.
