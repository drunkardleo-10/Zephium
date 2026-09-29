# Windows extensions: current QA handoff

Branch: `chore/windows-extension-probe`. This is an isolated Windows QA build;
main remains unchanged pending review and acceptance. Qualified on Windows 11,
WebView2 154.0.4258.37, with debug binaries.

## Run QA

From the repository root in normal, non-administrator PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File desktop\run-webext-qa.ps1 -Build
```

Omit `-Build` for subsequent launches. Close this checkout's QA app first.
The launcher verifies the **Zephium Extensions QA** identity and launches
`target\webext-qa\Zephium Extensions QA.exe`. Data belongs exclusively to
`%APPDATA%\app.zephium.webext-qa`. Execution policy is changed only for that
PowerShell process. See [the QA checklist](windows-extension-qa.md).

## What works

- Shared desktop registry, store review/download/verification, prepared packages,
  install, enable, disable and removal. Native WebView2 operations use the human
  profile's exact environment and a live management controller. Signed CRX IDs
  are preserved; keyed ZIP/folder packages work. MV2 and keyless packages fail
  before native loading.
- Native workers/content scripts; existing toolbar tiles observe title, badge
  and icon without polling. Popups use a small anchored native window and the
  15-line popup-only native-query binding. Bitwarden's welcome popup renders;
  signed-in vault/autofill acceptance remains manual.
- A focused, live Windows extension popup can open an ordinary browser tab
  through WebView2's original new-window request. The host validates the exact
  popup, runtime, profile environment, active human tab and foreground window,
  then uses the existing bounded native adoption path. It rechecks popup focus
  after construction. Hidden observers require a recent explicit action/options
  command.
  Grammarly 14.1333.0 completes sign-in in QA. The owner confirmed its popup
  recognizes the account and remains signed in after a full QA restart.
- Native context-menu entries named `extension` are preserved, including their
  native submenus and command dispatch. The previous allowlist removed them.
  Ordinary unknown commands and custom host entries remain filtered. An
  account-free Bitwarden lab profile confirmed the native extension menu entry.
- All sites and Specific sites use immutable prepared manifests, intersecting
  original host permissions and content-script matches. Native content-script,
  scripting and network-rule host restrictions were checked. A fixture's
  `chrome.storage.local` value survived All sites -> Specific sites (`example.com`)
  -> All sites in the actual QA UI. The owner clicked Save; subsequent values
  were read from the popup. No account credentials were used or copied.
- Same-ID replacement additionally passed a native lab test: the narrowed
  manifest became active, stored data survived, the formerly allowed localhost
  stopped receiving content scripts, and allowed 127.0.0.1 still received them.
  Reproduce with `crates\zephium-webext-windows\run-access-transition.ps1`.
  Evidence: `target/webext-access-transition/20260928-225337/results.jsonl`.
  There is no native Remove in access changes and no storage-loss warning is
  needed for this tested path. This does not establish every extension's own
  migration or signed-in storage behavior.
- Ordinary package, identity and add/enable failures settle only that install
  as Failed with Retry. The actual QA app rejected a deliberately invalid CSP
  with AddBrowserExtension `0x80004005`. Retry repeated the local failure;
  existing tabs/extensions remained usable, including a normal page reload.
  Quarantine remains for failed native startup identity/isolation attestation
  or inability to disable a partially loaded extension during rollback. Explicit
  close failures retain cleanup debt; native disable/removal failures still
  fail closed because revocation cannot be proven.
- Clean close/reopen and enabled-install restoration were verified. Work's
  adapter retains the extension-free automation subprofile. Its boundary now
  deliberately admits exactly one pre-navigation attestation hook, requires
  automation identity and absence of third-party extensions, and continues to
  forbid loading/enabling extensions. Work behavior itself was not expanded.

## Management-view measurements and budget

Fresh disposable lab profiles, one fixed loopback human page, same cumulative
packages: Bitwarden; + Dark Reader and Grammarly; + Vimium and SponsorBlock.
Each case idles for 120 seconds. Values below are mean private memory in the
last 30 seconds of sampling for the entire lab process tree, not the full QA
shell. No worker debugger is attached; CDP target inventory runs only before
and after idle. These are one-machine comparisons, not release CPU/battery
benchmarks or guaranteed upper bounds.

| Installed extensions | Persistent page per extension | One shared page parked on about:blank | Processes (persistent/shared) |
| --- | ---: | ---: | ---: |
| 1 | 227.0 MiB | 233.2 MiB | 9 / 9 |
| 3 | 352.8 MiB | 341.3 MiB | 11 / 11 |
| 5 | 405.5 MiB | 361.7 MiB | 13 / 12 |

Both five-extension cases ended with the same three workers: Bitwarden, Dark
Reader and Grammarly. Vimium and SponsorBlock workers retired even with their
management documents open. Persistent documents do retain renderer resources;
sharing saved about 44 MiB (11%) and one process at five, not one process per
extension. The shared trial loaded each host page then navigated the single
controller to about:blank; it did not implement production action refresh.

Retain eight running extensions process-wide for this QA: this reserves eight
observer resources and one popup without consuming browsing/Work/teardown pools.
Five already cost about 406 MiB in the minimal lab (179 MiB above the one-extension
case), so removing admission bounds is not justified by these measurements.
Eight is a provisional capacity choice with headroom over the measured five,
not a WebView2 limit or an experimentally proven safe maximum. The shared design
has measurable savings but loses asynchronous action-change observation while
parked; a production switch would need bounded refresh scheduling and stale-
action qualification. That tradeoff is left for review rather than adding a
second runtime to this QA patch.

Reproduce with `crates\zephium-webext-windows\run-management-resources.ps1`.
Raw comparisons: `target/webext-management-resources/20260928-223433`.
The first one-extension shared run had a 91.5-second sampling gap and is replaced
in the table by the uninterrupted repeat at `20260928-225018`. The runner reports
long gaps and supports `-Counts 1 -Modes shared` for exact case repeats.
Full-app ten-minute continuous CPU qualification remains open; the earlier QA
sample had a gap. Use `desktop\measure-webext-qa.ps1` for a fresh fixed-tab run.

## Limits and open acceptance items

- Grammarly's interactive login requires a native tab ID. WebView2 returned
  `undefined` from both forms of `tabs.create`, including with native `Allow`;
  this was not specific to our deferred child adoption. A worker-only wrapper
  now returns the FIFO native `tabs.onCreated` result, preserving native errors
  and removing its temporary listener after settlement or a 15-second timeout.
  Promise/callback same-URL concurrent creates passed in a disposable fixture.
  Completed Grammarly login and persistence after restart passed owner QA.
  No `identity.launchWebAuthFlow` replacement was added.
- Buttons without a popup now relay an explicit toolbar click through the
  extension's own host page to registered worker `action.onClicked` listeners.
  Native listeners remain registered. This does not synthesize activeTab grants
  or permission prompts. 1Password 8.12.37.1's cleared-popup action opened its
  own setup document in visible- and hidden-manager lab runs. The owner now
  reports it working in rebuilt QA; completed sign-in, filling and repeat-click
  acceptance have not yet been confirmed individually. Temporary click tracing
  was removed from the clean build. The earlier inert click did not reproduce
  after rebuilding; no isolated cause for that transient failure is established.
- Normal Windows tabs admit extension document URLs only with a live grant for
  that enabled install in the same profile. Disable/removal revokes the grant;
  stale view tokens and other profiles remain denied. Chromium enforces
  web_accessible_resources: a web-initiated public fixture navigation passed,
  while its private document was blocked. Options pages use the same native
  tab path. Fresh/restored tab construction also checks the live profile grant,
  including before warm-spare adoption. Full restart restoration of an active
  extension tab remains an owner QA check. No separate document server or
  extension-specific URL is used.
- Hidden manager new-window requests require a recent explicit action/options
  command (five seconds, up to eight requests) and the live foreground human
  tab. Ordinary unprompted manager opens remain denied. Popup-origin requests
  retain the exact focused-popup and environment checks.
- Dark Reader content works, but its background chooses a protected about: page
  for per-site popup controls despite the corrected popup query. On click site
  access, runtime optional-permission UI and native messaging remain withheld
  or unqualified. The extension-count limit of eight remains a pre-release item.
- Cross-profile acceptance, signed-in Bitwarden/autofill, full catalog behavior,
  complete disk-residue removal, update permission escalation, and sustained
  foreground/resource testing remain manual QA work. The separate Work product
  is not enabled or qualified by this build.
- Occasional action-refresh rejections on the internal manager retain prior
  presentation. Concurrent installed Zephium can own the global shortcut;
  QA does not take it over.

## Checks

Whole xtask suite: **154 passed**, including the revised Work boundary and
negative mutations. Engine with Work feature: **277 passed** under normal-user
execution (the restricted sandbox run had three filesystem failures, retained
in its separate log). Core: **211 passed**; webext: **53 passed**; app: **318 passed**;
worker compatibility JavaScript: **5 passed**. Desktop's preceding run:
**106 passed, one existing ignored test**.
QA desktop clippy passes with `-D warnings` using its required configuration;
three pre-existing vendored Wry dead-code warnings remain. The QA frontend
production build passes. No frontend behavior changed in this review follow-up.
The preceding shared extension/frontend/component results are in commit history;
these checks are not a claim that all workspace CI or release QA is green.

Reproduce the September 29 sign-in/menu/1Password findings with
`crates\zephium-webext-windows\run-browser-surfaces.ps1` after rebuilding the lab.
It requires cached signed Grammarly, Bitwarden and 1Password packages, uses
fresh profiles, and submits no account credentials. Its DOM click is diagnostic,
not evidence of physical user activation. Native document-load events establish
arrival at the Grammarly sign-in page; they do not establish OAuth completion.
`crates\zephium-webext-windows\run-worker-compat.ps1` checks native promise/callback
tab IDs and action relay against an account-free fixture. Completed Grammarly
authentication and restart persistence were checked separately by the owner.
