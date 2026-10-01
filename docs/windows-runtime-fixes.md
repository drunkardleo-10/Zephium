# Windows extension runtime qualification

Base: main at 607cd1af. Branch: fix/windows-runtime. Qualification: 2026-10-01 on a real Windows desktop with installed WebView2.

## Changes

- Hidden action hosts and popups register native process-failure observers before navigation. Host renderer recovery reloads the owned host.html with 250 ms, 1 s, and 4 s backoff, capped at three attempts per 60-second window. Recovery coalesces on the reserved lifecycle queue and checks the exact runtime generation and live controller. A popup renderer failure closes its controller. Browser-process failures retain the existing profile recovery path.
- Every Windows tab view, including warm spares, receives the page-close handler. Non-opener tabs require a committed, currently granted extension document; the shell also requires that extension to be running in the same profile. Revoked grants and ordinary web documents remain denied.
- Global or absent icons no longer trigger worker snapshots on tab changes. The worker reports whether it has per-tab icons, preserving those updates. Existing bounded icon-read retries remain in place. Package preparation hashes both changed scripts, so existing installs rebuild from their originals automatically.
- Startup admission uses stable profile/install ordering. Capacity-only failures wait for a freed slot; replacing a running extension keeps its slot. Other package errors do not enter an automatic retry loop.
- The existing install review warns when another enabled extension would exceed the global Windows limit of eight. Updating an already enabled extension does not produce this warning.
- Removal closes any open popup before acquiring the temporary removal controller. Errors before native extension mutation do not by themselves quarantine the profile. Independent native identity/cleanup failures still fail closed.
- A failed popup close transfers ownership of its native parent to the cleanup obligation. Admission follows a weak reference, so a successful retry releases the parent and allows subsequent popups. Unresolved cleanup continues to retain the parent and resource reservation.

The shared frontend is unchanged. The existing sleeping-tab styles_missed implementation is unchanged and exercised by the native qualification.

## Worker snapshot comparison

Controlled JavaScript fixture, 20 alternating tab switches, identical harness against the base script and this branch. These are worker snapshot calls, not measured CPU, battery, or page-load improvements.

| Icon case | Base | This branch |
| --- | ---: | ---: |
| Global icon | 20 | 1 |
| No icon | 20 | 1 |
| Broken icon | 20 | 2 |

Real WebView2 qualification also instruments the actual host sendMessage path. The missing-icon case accepts one initial snapshot plus at most one worker-start retry. The broken-icon case injects only the response's icon path while retaining native RPC and fetch/decode behavior.

## Verification

Results: 664 Rust unit tests passed (three opt-in tests excluded from the ordinary run); 22 JavaScript tests passed; the native qualifier was run separately; the production cargo check passed.

The opt-in native test uses disposable owned profiles and production engine paths. It deliberately crashes its own controllers through Page.crash, without opening a remote debugging port. It exercises host reload, native tab switches, extension-page close before and after package replacement, actual tab suspension/resumption with changed blocker CSS, popup crash, one failed controller close followed by a successful retry, disabled removal with eight managers and an open popup, pre-mutation capacity failure, and admission after unloading a manager. It finishes through the production process-group shutdown barrier, then launches a fresh test process using the same disposable persisted profile to verify extension-page close after restart.

The exact-controller close failure hook is enabled only through the engine's Windows dev-dependency. The desktop normal/build dependency graph contains no windows-cleanup-qualification feature. No production timer, debug endpoint, or failure injection is added by the qualifier.

Shell/engine tests cover deterministic admission, retry after disable/removal, replacement while at capacity, bounded recovery, and close authority. JavaScript tests cover global/per-tab icons, icon failures, concurrent updates, native tab identity, popup sizing, and the worker relay.

Commands (PowerShell, repository root):

~~~powershell
$env:CARGO_PROFILE_TEST_OPT_LEVEL = '2'
cargo test -p zephium-engine -p zephium-app -p zephium-desktop --lib
cargo test -p zephium-engine -p zephium-app -p zephium-desktop --lib native_windows_runtime_recovery -- --ignored --nocapture --test-threads=1
node --test crates/zephium-webext/src/windows/action-host.test.mjs crates/zephium-webext/src/windows/action-observer.test.mjs crates/zephium-webext/src/windows/worker-compat.test.mjs crates/zephium-webext/src/windows/popup-size.test.mjs
cargo check -p zephium-engine -p zephium-app -p zephium-desktop
~~~

Qualification boundaries: this is native engine and shell verification, not a full desktop UI click-through or saved-session UI restore test. Native process restart and persisted extension restoration are exercised directly; restored tabs share the tested ordinary-view construction/close path. A synthetic failing Close exercises cleanup handling; an arbitrary real WebView2 fault is not guaranteed to return the same error. No aggregate startup/RAM/battery/page-load claim is made. Raw logs, generated packages, profiles, and screenshots are not committed.
