# Chrome extensions on Windows: handoff

## Implementation follow-up — 2026-09-28

The owner approved proceeding past step 1 toward an isolated Windows QA build,
with local commits and a branch push only when QA is ready or a bounded blocker
is reached. Windows initially offers All sites and Specific sites; On click is
withheld. Work must use the automation subprofile in extension-enabled environments.

Visible popup lab evidence: `target/webext-visible-popup/20260928-170807`.
With a focused host-opened popup, both currentWindow and lastFocusedWindow return
the popup in Bitwarden and Dark Reader. Explicitly focusing the human controller
makes lastFocusedWindow return the human page; currentWindow remains the popup.
A 15-line popup-only query wrapper, given a native tab/window binding by the
host, redirects active-tab queries correctly in both extensions, including
callback/promise calls through the original native query. The lab obtains its
binding from the exact human controller using native `Browser.getWindowForTarget`.
Its window ID agrees with `chrome.tabs.query({windowId})`; no URL-based lookup is
needed. Native identity evidence: `target/popup-identity.jsonl`, with the same
binding also exercised by the prepared-package probe below.

Bitwarden's onboarding page executes. Dark Reader's UI still says protected
`about:` even though direct popup queries are corrected; its background-side
selection is not repaired by this popup-only wrapper. No worker tab dispatcher
or permission replacement was added. This is a targeting feasibility result,
not a claim that Dark Reader's full popup workflow works. Continue integration
as requested, retaining this limitation for QA.

The first Windows QA implementation is complete. Desktop registry, store downloads, staging,
updates and management are shared with Windows. Signed CRX identity is retained
in the unpacked manifest. Windows preparation produces immutable access-specific
packages; an end-of-preparation marker prevents reusing a partially written tree.
Specific sites intersects required/optional hosts and content-script matches,
retains original scheme/path constraints, removes `activeTab`, and replaces
`declarativeNetRequest` with Chromium's host-access-required variant. The latter
is covered by a manifest test and a native network-rule qualifier described below.
On click is hidden on Windows and rejected by the desktop command.

Prepared Dark Reader 4.9.133 evidence:
`target/webext-prepared/20260928-174640/results.jsonl` (runtime 154.0.4258.37).
The allowed `127.0.0.1` page receives Dark Reader styles; denied `localhost`
does not, and native `scripting.executeScript` is refused there. The automation
subprofile has only the two runtime components and receives no Dark Reader
content. An owned extension page reports native title, badge, popup, enabled
state and a 4096-byte RGBA icon. Native disable stops injection on a new document,
enable restores it, and remove stops it again. Reinstalling the same ID finds
neither the test `chrome.storage.local` value nor the DOM localStorage value.
This is not a complete disk-residue or signed-in storage qualification.

The native engine integration was qualified for this first QA slice in the distinct
`app.zephium.webext-qa` / **Zephium Extensions QA** app, whose Windows data is
under `%APPDATA%\\app.zephium.webext-qa`. The default WebView2 profile reports an
empty API profile name on this runtime, despite using a `Default` storage folder;
the startup attestation uses that observed identity. Native controllers remain
subject to environment/path/private-mode checks and cleanup-debt accounting.
Native load now obtains its Profile7 interface from an owned, live management
controller. A bootstrap controller's interface becomes invalid after close
(`0x8007139F`); keeping that COM reference alone was insufficient. Enable, disable
and remove run before closing their owning controller. Removal of a disabled
install uses a temporary accounted controller. The Wry startup gate remains in
place. Work selects the extension-free automation subprofile.

The actual QA app passed store download/review/install for Dark Reader and
Bitwarden, native enable/disable, Dark Reader removal, action icon/title
presentation, and host popup opening. Bitwarden reaches its welcome screen;
no account was used. Dark Reader retains the documented background-selection
limitation. Popup queries use the lab's exact native controller binding. Popups
use the owning browser window, fit a 400-by-600 client area, clamp to the monitor
work area and dismiss when their human tab navigates or loses active ownership.
Helper views deny unmanaged child windows. Shutdown disables native workers
before closing their controllers; the desktop registry restores requested
enablement on the next launch.

During qualification an earlier QA process failed full WebView2 group shutdown
and retained its storage lock. Only that disposable QA process tree was stopped;
the installed browser was left untouched. The corrected build subsequently
closed with an extension popup open: its app process and every QA WebView2 child
disappeared, with no shutdown failure. Reopening the same QA directory succeeded
and restored enabled Bitwarden. Disabling that restored install and then removing
it also passed in the actual QA app, exercising the temporary-controller removal
path. The manager settled to its empty state without a removal error.

The QA log still records occasional action-refresh rejections while navigating
the internal manager; the shell retains prior presentation state. The tested
human-page tiles and popup work, but this diagnostic remains follow-up work.
Running beside the installed browser also reports the already-registered global
shortcut; the QA window remains usable without taking over that shortcut.

The lab's reproducible network-rule qualifier is
`crates/zephium-webext-windows/run-host-rules.ps1`, using the gated lab binary and
the same neutral package preparation. Evidence:
`target/webext-host-rules/20260928-192313/results.jsonl`. Before installation a
loopback fetch succeeds. A prepared declarative blocking rule then blocks the
granted `127.0.0.1` host; the identical fetch succeeds on denied `localhost`.
All three results passed on runtime 154.0.4258.37.

Use [the Windows QA guide](windows-extension-qa.md) for the isolated launcher,
acceptance checklist and explicit limits. First QA supports signed store/CRX
packages and keyed ZIP/folders; keyless unsigned packages are rejected before
native loading. Options UI, on-click access and non-popup dispatch are withheld.
The existing Windows catalog no longer claims all listed extensions are verified.

Checks completed so far: 53 shared extension tests, five focused Windows shell
extension-browser tests, frontend check (295 unit tests), six ExtensionRow
Chromium component tests, frontend build, and the 39-file emitted-style audit.
Engine unit tests now execute after the separate activation-manifest fix. All
276 engine tests with Work enabled passed. Two saturation tests now fill the
Windows Work-terminal reserve, matching production's unchanged queue limits.
The existing download file-identity test failed in an earlier 199-pass/1-fail
run and passed in the full Work-enabled run; that earlier failure remains
recorded rather than being hidden by weakening its assertion.
Enabling the dormant Work feature exposed missing `Cancelled` event arms;
a separate fix settles only the exact native navigation, once, and its focused
regression test passes. Desktop unit tests also needed the Common Controls
activation manifest; all 106 tests then passed, with one existing ignored test.
Desktop/engine QA clippy passes with `-D warnings`; the vendored Wry dependency
still emits its three existing dead-code warnings. Node 24.18.0 and pnpm 11.17.0 are installed under ignored
`target/windows-tools` and the frozen frontend lockfile installs successfully.

Product resource evidence (debug build, one Example Domain page plus the
Extensions manager, popup closed):
`target/webext-qa-resources/20260928-191849-disabled-baseline/samples.csv` and
`target/webext-qa-resources/20260928-191953-bitwarden-idle/samples.csv`.
The disabled baseline ended at 604.84 MiB private memory and 21 processes.
With Bitwarden enabled, minute 2 through minute 8 samples were 685.71–694.66 MiB
and 22 processes, with a 696.99 MiB final sample. The requested 600-second run
actually spanned 1214.8 seconds and had a 731.5-second sampling gap; its continuous
ten-minute CPU qualification is therefore incomplete. The uninterrupted late
samples averaged about 5.1% of one core, but the short disabled CPU baseline and
sampling gap prevent a reliable CPU-overhead comparison. These include the
entire QA process tree, not the installed browser or the lab. Summed working
sets double-count shared pages; private bytes are the comparison above.
The earlier lab's ten-minute run remains separate evidence. The sampler now
warns explicitly on long gaps. No debugger was attached during these idle runs.

The remaining sections preserve the original handoff and step-1 observations.
Their descriptions of macOS-only product code are historical; the implementation
status above supersedes them. The archive branch mentioned below is unavailable
on GitHub and was not used. This document is the brief for that work: what
exists, what can be reused, what WebView2 gives and lacks, and how to verify.

## Goal and rules

Windows reaches parity with macOS for the same user-facing feature: install
from the Chrome Web Store (browser button on a listing, never injected into the
page), from a `.crx`/`.zip`/folder or by dropping one on the window; enable,
disable, remove with data erased; per-site access (all sites, on click,
specific sites); options pages; toolbar buttons with badges and popups; Web
Store updates, held back when they ask for more access; the runtime permission
prompt; the verified catalog.

The product rules that shaped macOS apply unchanged:

- Support only what is cheap. Never build something extraordinary for one
  extension. Ad blockers and to-do extensions are low priority (Zephium has
  its own blocker and tasks).
- Performance and resource use are first-class: instant launch, low RAM, low
  battery. Measure; do not assume.
- Tests yes, not overdone. Conventional Commits, small logical commits, no AI
  co-author trailer, comments only for non-obvious decisions.
- UI is shared: the Svelte frame is the same on both platforms. Do not fork it;
  add what Windows needs behind the existing domain/IPC.

## What exists

| Layer                                                                                                                                  | Path                                                                 | Platform                                |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- | --------------------------------------- |
| CRX3 verify, archive extraction, manifest, permission warnings, Web Store URLs and update protocol, compat injection                   | `crates/zephium-webext`                                              | neutral                                 |
| WebKit runtime, compat layer (`src/compat/compat.js`), native messaging, worker WebSocket bridge, offscreen documents, worker lifetime | `crates/zephium-webext-macos`                                        | macOS                                   |
| Engine host for extensions                                                                                                             | `crates/zephium-engine/src/host/webext.rs`                           | macOS today                             |
| Shell: which installs are loaded per profile                                                                                           | `crates/zephium-app/src/shell/webext.rs`                             | neutral                                 |
| Desktop: registry on disk, install/update/remove commands, catalog icons                                                               | `desktop/src/webext/` (`imp/` is `cfg(target_os = "macos")`)         | mostly neutral code behind a macOS gate |
| Frame UI: Extensions page, tray, review sheet, access prompt, catalog                                                                  | `frame/src/features/{webext,extensions}`, `frame/src/domain/webext`  | shared                                  |
| Real-extension suite                                                                                                                   | `cargo xtask webext-suite`, `crates/zephium-webext-macos/suite.json` | macOS lab                               |

On Windows every `web_extension_*` command currently returns "Extensions are
unavailable in this build".

The previous extension stack (about 260k lines, removed on 2026-09-28) is on
branch `archive/extensions-codex-2026-09-27`. It had WebView2 calls worth
reading, not copying: `crates/zephium-engine/src/platform/windows/extensions/native.rs`
(`ICoreWebView2Profile7::AddBrowserExtension`, `GetBrowserExtensions`,
`ICoreWebView2BrowserExtension::Remove/Enable`) and
`docs/extension-windows-validation.md`.

## What WebView2 gives and lacks

WebView2 runs real Chromium extensions: MV3 service workers, content scripts,
`chrome.storage`, `declarativeNetRequest` and most extension APIs work as in
Chrome, so WebKit's compat layer is not needed. Extensions must be enabled when
the environment is created (`AreBrowserExtensionsEnabled`), and are added per
profile from an unpacked folder.

What it lacks is the browser around them, and that is the work:

- No toolbar: the host gets no action icon, badge, title or click API, and no
  popup UI. Popups must be opened by the host (the action's `default_popup` in
  a small WebView2 of the same profile), and icon/badge/title changes must be
  observed some other way.
- No tab model beyond what WebView2 infers: verify what `chrome.tabs`,
  `chrome.windows` and `activeTab` report with Zephium's many WebView2
  instances, and whether a popup opened by the host sees the right active tab.
- No permission prompts, options UI, or install UI: all host-owned (the frame
  already has them).
- Native messaging (1Password, Bitwarden desktop): verify whether WebView2
  supports it; if not, it is out of scope unless cheap.

Treat every line of this section as a hypothesis to confirm on the machine
first (step 1 below); WebView2 has changed quickly.

## Plan

1. **Probe (a day, before any product code).** A throwaway harness, like the
   macOS `webext-lab` (`crates/zephium-webext-macos/src/bin/webext-lab.rs`),
   that creates a WebView2 environment with extensions enabled, adds a few
   unpacked extensions (Dark Reader, Bitwarden, Vimium, Grammarly, SponsorBlock
   from `target/webext-suite/*.crx` after extraction) and records: do they load,
   do content scripts run, does the worker start, what `chrome.tabs.query`
   returns, whether a host-opened `popup.html` works and sees the active tab,
   how action icon/badge changes can be observed, native messaging, memory per
   extension and per tab, worker lifetime. Write the findings into this file.
2. **Make the desktop layer neutral.** Move what is not WebKit-specific out of
   `desktop/src/webext/imp` (registry, install/download/verify/stage, updates,
   list, set enabled/access, uninstall, catalog icons) so both platforms share
   it. The compat preparation step stays macOS-only.
   `zephium_webext_macos::compat::CHROME_VERSION` is used for Web Store URLs;
   give it a neutral home.
3. **Engine host on Windows.** Load/unload/remove per profile through
   WebView2 in the same environment as the profile's tabs, keyed by the same
   `WebExtensionLoad` the shell already sends. Site access maps to host
   permissions the way `desktop/src/webext/imp/mod.rs` builds `match_patterns`;
   check how WebView2 applies runtime host permissions.
4. **Toolbar and popups.** Feed `ExtensionActionsView` (the frame already
   renders tiles, badges and the rail stack) from whatever step 1 found, and
   open popups anchored like macOS (`crates/zephium-engine/src/host/webext.rs`,
   `present_popup`). If icon/badge changes can only be observed from inside the
   extension, a small injected shim is acceptable; keep it minimal.
5. **Erase on remove.** WebView2 removal plus clearing the extension's origin
   data in the profile; verify nothing is left in the user data folder.
6. **Suite.** Extend `cargo xtask webext-suite` to run on Windows against the
   same `suite.json` expectations (start, popup renders, `check` page effects).

## Resource findings from macOS to re-check on Windows

- WebKit relaunched a background every minute for extensions with a one-minute
  alarm, and Bitwarden re-injected its scripts into every tab each time; the
  browser's memory climbed on an idle machine until restless workers were held
  loaded (`crates/zephium-webext-macos/src/lifetime.rs`). Chromium keeps
  workers alive differently; measure idle memory and CPU over 30 minutes with
  Bitwarden, Dark Reader and Grammarly on.
- Extension console output is forwarded only in development builds.
- Per open page, content scripts cost memory in the browser process too
  (Grammarly about 18 MB per page on macOS); per-site access "On click" is the
  user's lever.

## Known Windows risks from main (not caused by this work)

- Agent (Work) contexts now reuse the profile's ordinary WebView2 environment;
  verify that enabling extensions on it does not expose extensions to agent
  contexts.
- Windows clippy fails on four lints (CI run 36418895001):
  `crates/zephium-notes/src/folder.rs:87` and `:131` (needless `mut`),
  `crates/zephium-engine/src/host/download_files_windows.rs:332` (useless
  conversion) and `crates/zephium-engine/src/host/downloads/platform_windows.rs:711`
  (large `Err` variant). Fix these first; they block the Windows CI job.
- The Windows frontend component job fails in the Notes panel tests (also on
  macOS) and in the sidebar shape, dock and tab-plate motion tests; the same
  set failed on main before the extension work.
- `cargo xtask check-engine-floors` fails because the WebView2 review date
  (2026-09-18) expired; update the floor after reviewing the runtime.
- `aws-lc-sys` does not cross-build the desktop for Windows from macOS; build
  on Windows.

## Verify

On a normal, non-elevated Windows user with the pinned toolchain:

```powershell
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm -C frame run check
pnpm -C frame run build
```

Then the QA checklist, in a separately identified QA build so it never touches
a real profile: install from a store listing and from a dropped `.crx`; the
review sheet; enable/disable; site access "on click" (Grammarly only works
after clicking its button); options page; popups for Bitwarden, Dark Reader,
SponsorBlock; remove erases data and the extension's process ends; a held
update asks again for new access; 30 idle minutes with memory flat.

## Windows probe work record — 2026-09-28

**Step 1 has reached its reporting stop: native loading works, but popup/tab
semantics, required-host revocation and Work isolation prevent a parity claim.
Product integration and desktop neutralization have not started.** The earlier sandbox
failures are retained as execution history, not extension capability findings.

### Approved scope adjustments

- Keep a reusable lab binary in `crates/zephium-webext-windows`, behind the
  `lab` feature, separate from product builds. Respect the existing Wry gate.
- Prioritize load/worker/content scripts, host-opened popup and active-tab/on-click
  behavior, action state, site access, Work isolation, then resource baselines
  and a **10-minute** idle run. The later full-product 30-minute QA remains a
  separate release qualification requirement.
- A roughly 200-line action-reporting/click-relay script is the workaround
  ceiling. Do not reimplement tabs, action dispatch or permissions. No such
  replacement API has been implemented. A 22-line diagnostic-only action-call
  observer tests reporting feasibility without changing native API behavior.
- Native messaging and removal-residue checks are optional within the timebox;
  both are **not tested**.
- The archive branch is unavailable and is not a dependency. The four baseline
  Windows lints are isolated in commit `41c2404f`, not mixed with lab work.

### Prepared harness and inputs

`webext-lab-windows` uses the existing `webview2-com` 0.38/Wry adapter, a fresh
data directory, explicit profile names and the authenticated Wry startup gate.
The gate checks the actual user data directory, profile name, private-mode bit
and identity of a reused COM environment. The binary is feature-gated and refuses
release builds. It has a loopback fixture, a diagnostic MV3 extension, JSON
scenarios, JSONL observations, popup screenshots and process-failure reporting.
`run-probe.ps1` starts with an extension-free renderer check and stops if that
baseline fails; it must not produce resource conclusions from a broken renderer.
See the crate README for commands and interpretation.

All five official CRXs were downloaded, signature-verified with
`zephium_webext::crx::verify`, matched to their expected IDs and extracted using
the existing bounded extractor. The signed developer public key is added to
the unpacked manifest to preserve the extension ID; original CRXs are unchanged.

| Extension | Version | Manifest | SHA-256 |
| --- | --- | --- | --- |
| Dark Reader | 4.9.133 | MV3 | `ee38f20d1d50789f4b482c9a1a5dadab599b8682fd2a7dfb7de21fae8561b918` |
| Bitwarden | 2026.9.2 | MV3 | `6d3087dab30d2154948058dae8e4c7e41420d2ac816851e7c68caa932416c930` |
| Vimium | 2.4.2 | MV3 | `3198c26aa719be462dea585050fbed9b8b80628d57ea88a113babf5334c5517c` |
| Grammarly | 14.1332.0 | MV3 | `450aefeaa91b08ddf8129ae64bc86da0a0a24b1666599447edb4d955942b9a2a` |
| SponsorBlock | 6.1.6 | MV3 | `5587fe2b6a9946101ca0dde7c6efe76a755c5ee5b6d6e63dfc48c7f736cdb4ff` |

Packages are in `target/webext-suite`; extracted verification copies are in
`target/webext-verified-<id>/<id>`. These are local evidence, not tracked source.

### Initial sandbox execution (superseded by normal-account results below)

Machine: Windows x64 build 26200; installed SDK 10.0.26100.0; Rust 1.95.0 MSVC.
The created WebView2 environment reported **154.0.4258.37**. Execution account:
`DESKTOP-157E6GB/CodexSandboxOffline`.

| Probe question | Evidence/status |
| --- | --- |
| Environment enablement and startup gate | **Observed working in sandbox.** An extension-enabled environment and named profile were created through the unchanged Wry gate. |
| Add/list diagnostic extension | **Observed working in sandbox.** `AddBrowserExtension` returned ID `agkefboimiopkbcpdhgojemijgnljcaj`; enumeration reported it enabled. This fixture ID is path-derived, not a store ID. |
| Basic page execution without extensions | **Failed in sandbox.** GPU process failures reported kind 6, reason 3, exit `-1073741790` (`0xC0000022`, access denied); render process failures reported kind 1, reason 4 (launch failed), exit 49. The browser subsequently exited. |
| Workers/content effects, popup/active tab, action state, access enforcement and Work isolation | **Blocked by baseline process failures.** Evaluation returned `0x8007139F` or timed out. These are not evidence that WebView2 lacks the extension features. |
| Five real extensions' runtime behavior | **Not tested.** Package verification/extraction succeeded; load and runtime compatibility are separate questions. |
| Memory/CPU baselines and 10-minute idle | **Not tested.** Measuring repeatedly failed processes would not establish extension overhead. |
| Native messaging and removal residue | **Not tested**, lower priority by approved scope. |

Diagnostic scenario output is in `target/webext-windows-sandbox-02.jsonl` and
the adjacent stderr log. The extension-free control was run with
`scenarios/baseline.json` into `target/webext-windows-baseline-01`; its process
failure events were captured in terminal output. All views were hidden; even a
successful hidden run would not prove real foreground action-click semantics.

An initial standalone launch failed before `main` with `0xC0000139` because the
lab lacked Common Controls v6 activation. The lab now embeds that dependency;
the executable starts, verifies packages and creates WebView2 environments.
This was a harness defect, not an extension-runtime finding.

### Initial build constraints (historical)

The sandbox's Cargo TLS backend fails with `SEC_E_NO_CREDENTIALS`; an offline
full-workspace resolution also lacks the index entry for
`objc2-authentication-services`. Normal-account execution from the agent was
rejected by the session permission policy. A standalone scratch manifest using
the actual lab source, vendored Wry and neutral package crate built successfully
offline after official crate downloads were checked against their lockfile
SHA-256 values. Lab-only Clippy passed with `--no-deps -- -D warnings`; vendored
Wry reports three existing unused-constructor warnings. This is **not** a
successful locked full-workspace build. Full lint/test validation of the
separate lint commit is pending dependency resolution.

Repository Rust version matches the machine. Frontend checks also require
aligning Node 24.15.0 to the pinned 24.18.0 and pnpm 11.19.0 to 11.17.0. Those
changes are not required to execute this Rust-only probe and have not been made.

Normal-account execution subsequently became available. The repository lab
built successfully with the pinned toolchain and `--locked`; the only lockfile
change adds this workspace member, with no dependency version changes. The
PowerShell script-policy error was resolved with a process-scoped policy:

```powershell
cargo build --locked -p zephium-webext-windows --features lab --bin webext-lab-windows
powershell -NoProfile -ExecutionPolicy RemoteSigned -File crates\zephium-webext-windows\run-probe.ps1
```

This does not change the user or machine execution policy. Do not condition the
retry on the previous failed invocation's `$LASTEXITCODE`.

### Normal-account findings

Evidence: `target/webext-windows-probe/20260928-155006`, normal non-elevated
`DESKTOP-157E6GB/user`, Windows 11 Pro x64 build 26200, WebView2
**154.0.4258.37**. Every scenario owns fresh disposable storage. The corrected
loopback server explicitly puts accepted Winsock sockets into blocking mode;
earlier runs with intermittent fixture connection errors are excluded.
PowerShell also retains each child's process handle so a successful short-lived
process is not misreported as having a null exit code.

| Question | Observation and limit |
| --- | --- |
| Environment, gate, renderer | Normal-account extension-free smoke passed. The existing Wry gate is unchanged; the lab verifies storage, profile identity, non-private mode and reused COM environment before navigation. |
| Package loading | All five signed MV3 packages load and enumerate as enabled with their verified store IDs. This establishes loading, not end-to-end functionality. |
| Workers | Each real extension has a service-worker target. Diagnostic content-to-worker messages return successfully. No debugger is attached to service-worker targets. |
| Content scripts | Diagnostic allowed-host content executes; denied `localhost` content does not. Dark Reader injects its style on the loopback fixture and passes its existing `example.com` suite check. Grammarly passes its existing `example.com` DOM check; its loopback marker is absent. |
| Host-opened popups | All five popup documents execute and return DOM text. Bitwarden shows onboarding, Grammarly sign-in/onboarding, SponsorBlock no-video text. Screenshots time out with hidden controllers, so visual rendering has not been qualified. |
| Tabs and active tab | Every hidden controller appears as its own window with one `active:true` tab. A direct `chrome.tabs.query({active:true,currentWindow:true})` from every real popup selects the **popup itself**, not the human page. Dark Reader and Vimium therefore report an unsupported/protected page in this setup. A worker query can select a different window. This is a blocker for the tested host-opened-popup strategy. |
| Action / on-click semantics | Native setters for title, badge and icon complete; title and badge read back correctly. Opening a popup by URL is not an action click. Real toolbar `onClicked`, transient `activeTab` grants and native user-gesture permission approval are **not established**. No tabs/dispatch/permission compatibility layer was built. |
| Site access | Diagnostic scripting succeeds on the granted host and rejects the ungranted host with the native manifest-permission error. A permission request without a gesture rejects. Cross-origin fetch also rejects, but this alone does not distinguish permissions from CORS. Product access modes are not qualified. |
| Work topology | A hidden view in `LabHuman` receives the extension content script and is visible in its tab enumeration. A view in separate `agent-lab` has neither that content marker nor that diagnostic extension installed (built-in extensions may still enumerate). Reusing the human profile is therefore not an isolation boundary. This reproduces profile topology, not the production Work adapter. |
| Extension-specific features | Bitwarden vault login/autofill, Grammarly authenticated features, Vimium keyboard behavior and SponsorBlock video skipping are **not tested**. The Vimium DOM selector used by the broad probe is insufficient to establish failure. |
| Optional checks | Native messaging and extension-removal residue are **not tested** in this timebox. |

Supplemental `access-and-actions.jsonl` confirms the diagnostic observer records
native `setTitle`, `setBadgeText` and `setIcon` calls (16×16 image, 1024 bytes).
This is a feasibility result inside the diagnostic worker, not general injection
into store extensions, icon rendering, or a native host notification channel.
The wrapper preserves original function results and does not dispatch actions.

The same run rejects `chrome.permissions.remove({origins:
['http://127.0.0.1/*']})` with **"You cannot remove required permissions."**
Subsequent navigation still runs content scripts and scripting still succeeds.
The lab records this as CDP `exceptionDetails` even though its transport operation
has `ok:true`. Runtime site-access withholding therefore remains unresolved;
this API call is not a working revocation mechanism for required host grants.

Worker lifecycle is **not deterministic in these short trials**. In
`worker-lifetime.jsonl` and `worker-lifetime-retry.jsonl`, the diagnostic
service-worker target disappears after 45 seconds without extension messages.
The retry later receives a successful content-script reply on a new navigation;
the first trial evaluated before that reply was ready, and the retry's initial
page evaluation timed out. With the corrected readiness wait in
`worker-lifetime-final.jsonl`, all evaluations succeed, but the worker remains
present and returns the same start timestamp after the idle interval. Worker
targets remain `attached:false`. These supplemental trials overlap build
validation and are not resource benchmarks. This establishes working worker
messaging and observed retirement, not a reliable exact shutdown deadline or a
reason to port the macOS keep-alive workaround. Preserve all trials as evidence.

### Resource measurement

The two baselines sleep for 60 seconds each; the three-extension scenario
completes a full **600,000 ms** idle step. There are 28 samples per baseline and
285 samples over approximately 607 seconds for the extension process family.
No `ProcessFailed` events occurred in this normal-account run.

| Three hidden loopback tabs | Late private memory, median | Processes | Late CPU, percent of one core |
| --- | ---: | ---: | ---: |
| Extensions disabled, no installed test extensions | 177.0 MiB | 10 | 1.88% |
| Extensions enabled, no installed test extensions | 179.8 MiB | 10 | 2.51% |
| Bitwarden + Dark Reader + Grammarly loaded | 351.1 MiB | 12 | 1.79% |

The table uses each run's last 30 seconds. Extension memory has a minute-two
median of 406.1 MiB (13 processes), then a final-minute median of 351.1 MiB
(12 processes; range 350.6–352.0 MiB). From 60 seconds onward the measured
processes consumed 10.58 CPU-seconds over approximately 546 seconds, equivalent
to 1.94% of one core. This run shows no sustained private-memory climb.
The late loaded-versus-enabled-baseline difference is about **171 MiB**, a
combined workload cost that cannot be assigned to individual extensions.

CSV totals include the lab and descendants, including Chromium utility/GPU
processes. Shared working sets are not used for the primary comparison because
summing them double-counts shared pages. CPU includes the lab's polling message
pump and fixture server; it is not a battery benchmark. Very short-lived
processes between samples may be missed. The original capture timestamps each
process row separately; `summarize-resources.mjs` groups adjacent rows into its
approximately two-second samples. The runner now stamps each batch once.

These are one-machine, one-run hidden-controller measurements on simple local
pages, without signed-in vaults, real video or Grammarly's demonstrated
`example.com` content workload. No CDP calls or worker debugger attachments are
made during resource idle. Per-extension/per-tab slopes, foreground interaction,
repeated trials and the separate **30-minute product QA** are not measured.

### Final validation and machine preparation

- `cargo build --locked -p zephium-webext-windows --features lab --bin webext-lab-windows`: passed.
- `cargo clippy --locked -p zephium-webext-windows --features lab --all-targets -- -D warnings`: passed.
- `cargo clippy --locked -p zephium-engine -p zephium-notes --lib -- -D warnings`: passed, validating the four isolated fixes at the library boundary.
- `cargo fmt --all -- --check`, JavaScript syntax checks and PowerShell parsing: passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: failed on
  the existing `items_after_test_module` lint in
  `crates/zephium-engine/src/host/download_files_windows.rs` (test module at
  line 623 precedes three functions). The ordering is present in starting
  commit `a5262816`. It is not changed as part of the probe.
- Initial `cargo test --locked --workspace`: stopped with 28 `xtask` failures
  caused by Git's system `core.autocrlf=true` checkout conversion. The failing
  inputs were `i/lf w/crlf`; several validators require exact LF text. Set
  **repository-local** `core.autocrlf=input` and normalized only tracked
  `i/lf w/crlf` files (1405 files), preserving all other bytes and existing
  edits. Git blob comparisons confirm no added source-content changes. No
  global Git setting or `.gitattributes` change was made.
- After LF correction, all **154 xtask tests pass**. The workspace test run
  advances and stops in `zephium-app`: **317 passed, 2 failed**. Failures are
  `shell::tests::extension_browser_requests::authenticated_browser_mutations_follow_shell_scope_and_settle_exactly_once`
  and `shell::tests::extension_browser_requests::delayed_create_reply_cannot_override_a_newer_failed_navigation_or_closed_tab`;
  both unwrap missing first-navigation values (lines 55 and 164). The relevant
  shell test and implementation files are unchanged from `a5262816`. These
  Windows extension-shell failures remain unresolved; the full suite is not green.
- Focused engine tests cannot start: the test executable exits with
  `0xC0000139` (`STATUS_ENTRYPOINT_NOT_FOUND`) before running tests. The lab's
  embedded Common Controls v6 manifest fix does not apply to this separate
  executable; its loader failure remains unqualified rather than counted as
  passing engine tests.
- Focused Notes tests: **30 passed, 1 failed**;
  `library::tests::the_file_name_follows_the_title_until_someone_renames_it`
  expects `groceries.md` but receives `Groceries.md` at `library/tests.rs:148`.
  No Notes filename behavior was changed by the isolated lint fixes.
- Neutral extension package tests: **49 passed**; the separate disk-corpus test
  remains ignored by default. The five probe CRXs were verified and loaded by
  the lab independently.

The three existing vendored Wry constructor warnings remain. Frontend checks
were not run: align Node/pnpm with repository pins before that work. The runtime
floor review, foreground QA, native messaging and removal-residue qualification
also remain outside this completed probe. Raw build/test logs are retained with
the native evidence, including the initial CRLF failure and corrected rerun.

The popup/tab and shared-profile findings must be resolved in design before
enabling extensions in production. A visible, correctly focused popup could
behave differently and remains untested; these hidden-controller observations
must not be promoted to a universal WebView2 compatibility claim. In particular,
do not silently solve these findings by rebuilding `chrome.tabs`, native action
dispatch or permissions. **Stop after step 1 and review the evidence.**
