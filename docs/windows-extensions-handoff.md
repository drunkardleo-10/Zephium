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
  after construction. Hidden observers still cannot open tabs through this path.
  Grammarly 14.1333.0's real OAuth request reaches its sign-in document in the
  disposable lab; completed account sign-in remains a user acceptance check.
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

- **1Password 8.12.37.1 is not usable yet.** It declares a default popup but
  clears the native popup URL during unsigned-in startup, then requests
  `chrome-extension://.../app/app.html#/page/welcome`. That document renders
  when explicitly opened in the lab; normal Windows tabs correctly reject its
  extension scheme. Its non-popup action and extension-owned document lifecycle
  need a separate implementation/review. `Extensions.triggerAction` and
  `Extensions.getExtensions` returned `0x80070057` in this WebView2 runtime;
  no native action path was qualified. Do not hardcode its internal welcome URL,
  force the cleared manifest popup, or add a replacement action dispatcher.
  This also explains some blocked-new-tab notices during installation/startup;
  a notice on a web page does not identify that page as the request's cause.
- Dark Reader content works, but its background chooses a protected about: page
  for per-site popup controls despite the corrected popup query. No background
  tabs/action/permissions emulator was added.
- On click access, non-popup action dispatch, runtime optional-permission UI,
  native messaging, and options pages remain withheld/unqualified. Normal tab
  navigation intentionally admits only HTTP(S) and exact about:blank; macOS
  owns a separate extension-document tab path. Options are therefore not a
  cheap manifest-URL routing change on Windows. Do not broaden the ordinary
  navigation gate just to expose them.
- Cross-profile acceptance, signed-in Bitwarden/autofill, full catalog behavior,
  complete disk-residue removal, update permission escalation, and sustained
  foreground/resource testing remain manual QA work. The separate Work product
  is not enabled or qualified by this build.
- Occasional action-refresh rejections on the internal manager retain prior
  presentation. Concurrent installed Zephium can own the global shortcut;
  QA does not take it over.

## Checks

Whole xtask suite: **154 passed**, including the revised Work boundary and
negative mutations. Engine with Work feature: **276 passed** under normal-user
execution (the restricted sandbox run had three filesystem failures, retained
in its separate log). Desktop: **106 passed, one existing ignored test**.
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
