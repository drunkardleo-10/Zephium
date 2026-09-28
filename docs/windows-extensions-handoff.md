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
