# Windows extensions: review handoff

Branch: `chore/windows-extension-probe`. **Zephium Extensions QA** now supports
optimized release builds. Main remains unchanged pending review and acceptance.
This is qualified Windows MV3 support, not compatibility with every Chrome extension.

## Run QA

In ordinary, non-administrator PowerShell, from the repository root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File desktop\run-webext-qa.ps1 -Build -Release
```

Close QA before building. Omit both switches for subsequent launches.
The executable is `target\webext-qa\Zephium Extensions QA.exe`;
`revision.txt` and `build-profile.txt` identify its source and build mode.
Data stays in `%APPDATA%\app.zephium.webext-qa`, separate from real profiles.
If packaged-parent AppData redirection triggers the Wry startup gate, launch
through ordinary PowerShell or File Explorer. Never weaken the gate or move
profile data to work around it. See [the QA checklist](windows-extension-qa.md).

## Implemented and checked

- Shared store review, signed download verification, registry, enable/disable,
  removal and retry. Native operations use the exact human-profile environment.
  Signed CRX identities are preserved; ZIP/folder installs require a manifest key.
  MV2 and keyless packages are rejected before native loading. Update responses
  and package downloads enforce size limits while streaming.
- Native workers/content scripts. All sites / Specific sites intersect original
  host permissions and content-script matches; content injection, scripting and
  network-rule restrictions have qualifiers. Same-ID replacement preserves
  `chrome.storage.local` in the lab and actual QA UI. Disabled packages skip
  compatibility preparation at startup and prepare on enable.
- Toolbar title, badge, icon and popup observation, with original-window binding
  across asynchronous tab switches. The host caches only the last package icon
  and active-tab raster; unchanged refreshes avoid worker messages and decoding.
  Updates are event-driven, with drift repair on the existing maintenance
  heartbeat, not a new polling timer.
- Anchored native popups with a small popup-only active-tab query binding.
  No-popup buttons relay explicit clicks to registered worker listeners without
  granting activeTab. The worker `tabs.create` wrapper supplies WebView2's missing
  result using FIFO native `tabs.onCreated` events, preserves native errors, and
  removes its temporary listener on settlement or a 15-second timeout.
  Promise/callback concurrent-create fixtures pass.
- Focused popup requests use bounded native tab adoption with runtime, profile,
  environment and focus checks. Hidden managers need an explicit action/options
  command within five seconds, at most eight requests, and a live foreground
  human tab. Unsolicited manager opens remain denied.
- Extension documents/options use normal tabs only for an enabled install with
  a live grant in that profile, including restored tabs and warm-spare adoption.
  Disable/removal revokes the grant. Chromium enforces web_accessible_resources;
  public/private fixture navigations were qualified.
- Native extension context-menu entries/submenus are retained. The store's Add
  to Chrome control is hidden on the exact HTTPS store origin, leaving Zephium's
  reviewed sidebar installation. English, Polish and unrelated-page negative
  checks passed. Internal pages intentionally omit action tiles.
- Package/ID/add/enable failures affect only that install (Failed / Retry).
  Native startup integrity failures, failed revocation, or failed disable during
  partial-load rollback fail closed. Cleanup debt remains accounted for.
  A deliberately invalid native package failed locally in QA without closing
  existing tabs or disrupting other extensions.
- Two-profile fixtures qualify native install/storage isolation and lifecycle.
  Work retains its extension-free automation subprofile and one bounded
  pre-navigation attestation hook; no Work product behavior is enabled here.

Owner acceptance: Grammarly completed login and stayed signed in after restart.
1Password received an aggregate "Yes, it works" for setup/sign-in/fill/repeat
click/tab restoration, not individual recorded results. Bitwarden installation
and popup usability were confirmed; signed-in filling and context-menu use were
not separately recorded. Vimium and Return YouTube Dislike worked in owner testing.
Dark Reader changes page content; its per-site popup limitation remains below.

## Performance evidence and decisions

Machine: Intel i3-1115G4 (2 cores / 4 logical processors), about 12 GB RAM,
Windows 11, WebView2 154.0.4258.37. Native lab binaries are debug; the release
QA sample below uses the optimized app. Private bytes cover the owned process
tree. Task Manager groupings and summed working sets are different metrics;
working sets double-count shared pages.

- Twenty steady action refreshes per extension fell from 20 icon fetches,
  decodes and worker requests to **one each**. Three-extension runs fell from
  roughly 168–169 ms to 29–37 ms per batch. This measures toolbar refresh work,
  not popup-open or page-load speed. Evidence:
  `target/webext-page-performance/20260929-195117-before-steady` and
  `20260929-195449-after`.
- Five warm local-page navigations (600 cards, roughly 3,600 nodes) with
  Dark Reader, Grammarly and Bitwarden had median load 244.5 ms after the change,
  versus 246.5–259.9 ms before. No reliable page-load improvement is established.
  After-run no-extension median: 57.1 ms; one Dark Reader: 184.8 ms.
  These measure extension work on a fixed fixture, not public-web performance.
- Five-extension normal/low-memory-hint ABBA trials were 460.3 / 418.4 / 414.2 /
  412.4 MiB at 60–80 seconds. The reduction did not reproduce against the second
  normal run. Hidden managers retain the best-effort low-memory hint without
  suspension, but **no RAM saving is attributed to it**.
- A two-minute release QA observation had 25 processes, 846.0–848.6 MiB private
  memory and zero sampled CPU in four intervals. Evidence:
  `target/webext-qa-resources/20260929-201044-optimized-release`. The earlier
  debug session was roughly 1 GiB, but tab residency differed after restart.
  That is not a controlled memory-saving comparison, battery-life result, or
  memory upper bound.
- Final ten-minute observation: 25 processes, 992.1–1040.4 MiB private memory,
  ending at 992.1 MiB; time-weighted CPU 3.83% of one core (about 0.96% of this
  four-thread machine). All 21 samples were contiguous (largest gap 30.36 s).
  The owner reported browsing with extensions disabled during this run, so it
  is **not an extension-idle qualification** or a comparable baseline. No Rust
  build/native lab ran during sampling. Evidence:
  `target/webext-qa-resources/20260929-220426-final-review-observational`.

The shared-management-view trial remains a design decision:

| Extensions | Persistent manager per extension | Shared manager parked on about:blank | Processes |
| --- | ---: | ---: | ---: |
| 1 | 227.0 MiB | 233.2 MiB | 9 / 9 |
| 3 | 352.8 MiB | 341.3 MiB | 11 / 11 |
| 5 | 405.5 MiB | 361.7 MiB | 13 / 12 |

Means cover the final 30 seconds of 120-second minimal-lab trials, not the full
browser. Both five-extension trials ended with the same three workers; idle
Vimium/SponsorBlock workers retired even with persistent managers. Sharing saved
about 44 MiB at five but loses asynchronous action notifications while parked.
Keep persistent observers until a replacement preserves correctness. Evidence:
`target/webext-management-resources/20260928-223433`; the interrupted one-extension
shared sample was replaced by `20260928-225018`.

## Remaining release decisions and acceptance

- Resolve the **eight running extensions process-wide** budget before public
  release: measure a higher supported count and adjust resource pools, or make
  a deliberate product-limit decision. Eight is provisional QA capacity,
  not a WebView2 limit or measured safe maximum. One popup is allowed at once.
- Dark Reader's worker can select a protected about: page for per-site popup
  controls despite the popup binding. Do not claim those controls work.
- On-click site access and runtime optional-permission UI are withheld.
  Native messaging / desktop companions are unqualified. MV2 is unsupported.
  No identity.launchWebAuthFlow replacement was added.
- Finish explicit signed-in Bitwarden filling/context-menu checks, whole-app
  cross-profile acceptance and a real permission-escalating update. Fixture
  storage preservation does not qualify every extension's migration. Full disk
  removal residue and the full extension catalog remain unqualified.
- Qualify sustained idle, repeated install/remove cycles, realistic page loads
  and cold/warm startup with fixed tabs/packages and release binaries.
  Battery savings require a controlled power measurement. A single Task Manager
  screenshot, or lower memory after lazy restoration, is insufficient evidence.
- Requalify the store selector when its markup changes and native paths when
  WebView2 changes. Rejected action reads retain prior presentation.

## Final review validation (2026-09-29)

Full xtask: **154 passed**. App: **318**; core: **211**; engine with
agentic-browser feature: **277**; webext: **53**; desktop: **110 passed, one
existing ignored test**, including four new HTTP fixture tests for body limits
and interrupted downloads; desktop configuration integration tests: **8 passed**.
The optional real-CRX integration test remains ignored (native qualifiers use
cached verified packages). Worker/action-host JavaScript: **13 passed**.
Strict desktop + Windows lab clippy, formatting and whitespace checks passed;
three pre-existing vendored Wry dead-code warnings remain.

Native qualifiers passed again: lifecycle/profile isolation
(`target/webext-lifecycle/20260929-221804`), same-ID storage/access replacement
(`target/webext-access-transition/20260929-221811`), host network rules
(`target/webext-host-rules/20260929-221817`), and worker tab-create/action relay
(`target/webext-worker-compat/20260929-221821`). These tests use disposable data.
Frontend behavior did not change in this pass; the previously built production
frontend is reused by the optimized QA rebuild. This is not a claim that all
workspace/platform CI or the remaining manual release gates passed.

```powershell
$env:TAURI_CONFIG = Get-Content desktop/tauri.webext-qa.conf.json -Raw
cargo test -p zephium-desktop -p zephium-engine -p zephium-app -p zephium-core -p zephium-webext -p xtask --features zephium-desktop/webext-qa,zephium-engine/agentic-browser
cargo clippy -p zephium-desktop -p zephium-webext-windows --features zephium-desktop/webext-qa,zephium-webext-windows/lab --all-targets -- -D warnings
node --test crates/zephium-webext/src/windows/action-host.test.mjs crates/zephium-webext/src/windows/worker-compat.test.mjs
cargo fmt --all --check
```

## Reproduction

Build `cargo build -p zephium-webext-windows --features lab`; the lab is excluded
from product builds. Run scripts through PowerShell with process-scoped
`-ExecutionPolicy Bypass`. They use disposable profiles and no credentials.

| Scope | Runner under crates/zephium-webext-windows |
| --- | --- |
| Enable/disable/remove and profile isolation | `run-lifecycle.ps1` |
| Storage-preserving All/Specific transition | `run-access-transition.ps1` |
| Allowed/denied network rules | `run-host-rules.ps1` |
| Native tab IDs and explicit action relay | `run-worker-compat.ps1` |
| Sign-in arrival / native menu / 1Password diagnostics | `run-browser-surfaces.ps1` |
| Live store UI and exact-origin restriction | `run-store-ui.ps1` |
| Page and action-work measurements | `run-page-performance.ps1` |
| 1/3/5 manager comparisons | `run-management-resources.ps1` |

Browser-surface checks need cached signed CRXs. Diagnostic DOM clicks do not prove
physical activation or completed authentication; credential checks remain manual.
Full QA sampling uses `desktop/measure-webext-qa.ps1`. Raw target directories are
local ignored artifacts; retain relevant sanitized results before cleaning.
The scripts and this summary are committed.
