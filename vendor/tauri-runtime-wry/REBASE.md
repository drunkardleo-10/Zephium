# `tauri-runtime-wry` fork update procedure

1. Freeze a clean Zephium commit. Record the old and proposed Tauri revisions,
   release notes, security notices, dependency changes, and lockfile digests.
2. Obtain the complete old-upstream-to-local and old-upstream-to-new-upstream
   diffs. Start from the exact new upstream source; do not overlay selected
   files onto this tree.
3. Disposition every `FORK.toml` patch set as reapplied, reworked, retired
   upstream, or intentionally removed. Review every creation message, result
   channel, registry insertion, fallible native call, reentrant callback, and
   cleanup obligation even if the patch applies cleanly.
4. Preserve the contract that detached handles are created only after an exact
   event-loop acknowledgement. Verify success is acknowledged after registry
   insertion, every handler error reaches the caller, absent parents report
   `WindowNotFound`, and dropped receivers do not panic.
5. Re-audit provisional bookkeeping independently from native effects. Roll
   back only entries whose reversal is proven safe. In particular, do not
   erase retained Web contexts or Linux protocol-registration markers merely
   because later WebView construction failed.
6. Preserve terminal registry draining. An accepted exit must seal admission,
   detach the complete registry before native destruction can pump callbacks,
   revoke Tao ID routing, drop child WebViews before their parent windows, and
   enter `ControlFlow::ExitWithCode(code)` only after the exact-once drain
   completes. A nested exit or unexpected registry borrow must remain
   fail-closed. Tao also emits `LoopDestroyed` when the deprecated
   `run_iteration` returns; that synthetic boundary must never drain live
   windows.
7. Keep the standalone Wry path patch. Regenerate this lockfile from this
   manifest and the workspace lockfile from the repository root; review both
   complete diffs and all changed sources and licenses.
8. Run standalone format, locked tests, strict Clippy, and release checks on
   Windows, macOS, and supported Fedora, followed by the full workspace matrix.
   Exercise native success, injected construction failures, missing/reentrant
   parent removal, dropped callers, startup, shutdown, and renderer crashes.
9. Update `FORK.toml` and `UPSTREAM.md` only after two maintainers have reviewed
   the full delta. Stable release still requires packaged cross-platform tests
   and external native-boundary review.

If any native ownership or cleanup result is ambiguous, retain the previously
reviewed fork. Never combine source from one upstream revision with an
unreviewed lockfile from another.
