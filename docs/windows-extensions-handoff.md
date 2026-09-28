# Chrome extensions on Windows: handoff

Status: 2026-09-28. Extension support is finished for macOS (WebKit) and not
started for Windows (WebView2). This document is the brief for that work: what
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

**Step 1 is incomplete: execution under the normal Windows account is pending.**
The observations below are from the restricted agent account, not production
qualification. No extension integration or desktop neutralization was started.

### Approved scope adjustments

- Keep a reusable lab binary in `crates/zephium-webext-windows`, behind the
  `lab` feature, separate from product builds. Respect the existing Wry gate.
- Prioritize load/worker/content scripts, host-opened popup and active-tab/on-click
  behavior, action state, site access, Work isolation, then resource baselines
  and a **10-minute** idle run. The later full-product 30-minute QA remains a
  separate release qualification requirement.
- A roughly 200-line action-reporting/click-relay script is the workaround
  ceiling. Do not reimplement tabs, action dispatch or permissions. No such
  workaround has been implemented in this probe.
- Native messaging and removal-residue checks are optional within the timebox;
  both are currently **not tested**.
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

### Observed native results and limitations

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

### Build/check status and next execution

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

Resume from a normal, non-elevated PowerShell in the repository:

```powershell
cargo build -p zephium-webext-windows --features lab --bin webext-lab-windows
if ($LASTEXITCODE -eq 0) {
    powershell -NoProfile -File crates\zephium-webext-windows\run-probe.ps1
}
```

Review the first workspace lockfile update, then use `--locked` on subsequent
builds. Review actual observations, resolve only lab defects or bounded probe
questions, append the normal-account findings here, and **stop after step 1**.
Do not infer extension parity or proceed to step 2 from this blocked run.
