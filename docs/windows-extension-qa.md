# Windows Extensions QA

Branch: `chore/windows-extension-probe`. This is an early Windows QA build,
not a main-branch release. Use a normal, non-administrator PowerShell.

From the repository root, with its pinned Node/pnpm tools and Rust installed:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File desktop\run-webext-qa.ps1 -Build
```

Subsequent launches of the existing build:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File desktop\run-webext-qa.ps1
```

The process-scoped execution-policy argument permits these local scripts without
changing machine policy. The launcher refuses a second live instance from this
checkout, verifies the binary's QA product identity, and launches
`target\webext-qa\Zephium Extensions QA.exe`. Its data belongs exclusively to
`%APPDATA%\app.zephium.webext-qa`; it does not import a normal Zephium profile.
`revision.txt` alongside the executable records the build revision.

If a launch from a packaged development tool reports that a WebView2 generation
escaped its owned runtime root, run the command above from ordinary Windows
PowerShell, or open the verified QA executable in File Explorer. Packaged-parent
AppData redirection can make the canonical child resolve into a package cache
while its parent resolves into Roaming. Keep the directory check intact and do
not copy, reset or delete profiles to work around this environment mismatch.

## First acceptance pass

1. Open a Chrome Web Store listing for Bitwarden or Dark Reader. Use the
   browser's **Add to Zephium** button, review permissions, and add the extension.
   Its row should settle to an enabled version without an error.
2. Visit an ordinary HTTP(S) page. Open **Utilities**, then the extension tile.
   Check its icon/title, the anchored popup, and close/reopen behavior. Bitwarden
   should reach its welcome screen. Signed-in vault/autofill testing remains a
   manual QA task; use a disposable test account.
3. Disable the extension, reload the page, and check that new content scripts
   stop running. Re-enable and reload. Dark Reader is a useful visible check.
   Existing script effects can remain until reload.
4. In the extension row's menu, select **On specific sites**, allow one test
   hostname, and reload both that host and a different hostname. Only the
   allowed host should receive content scripts. Switch back to **All sites**.
5. Remove the extension, reload the page, and restart QA. Its row, action and
   injection should stay absent. Reinstall and check a fresh extension state.
6. Repeat with a second browser profile. Extension enablement and storage should
   remain separate. Private profiles do not support extensions.

## Current limits

- The new worker compatibility wrapper supplies missing native tab-create
  results and relays actions without a popup. Extension-owned pages and options
  now use normal profile-checked tabs. These paths passed account-free lab
  checks, including 1Password's setup action. Owner acceptance is recorded below.
- Grammarly completed login and remained signed in after a full QA restart,
  confirmed by the owner. Recheck this after subsequent compatibility changes.
- The owner confirmed the clean-build 1Password checklist with "Yes, it works":
  opening, sign-in, filling, another toolbar click, and extension-tab restoration
  after restart were requested together. No per-step results were supplied.
  Desktop companion integration is separate and remains unqualified.
- Native extension context-menu entries are now retained. For Bitwarden,
  right-click a login field and check its submenu. Menu availability alone does
  not establish that credential filling or inline suggestions work.
- Popup-only native active-tab query binding works in the lab for Bitwarden and
  Dark Reader. Dark Reader's background logic still selects a protected `about:`
  page; its per-site popup controls are not qualified. Its content scripts work.
- On click site access and the host runtime permission prompt remain unavailable.
  The action relay does not synthesize Chromium activeTab permission grants.
- Store installs and signed CRX files retain their verified Chrome identity.
  ZIP/folder installs require a manifest key; keyless packages are rejected
  before native loading. Manifest V2 is not supported.
- Up to eight extensions can run across this Windows process, with one popup.
  Hidden observer pages use a separate bounded native-resource pool. This is a
  provisional QA budget, not a WebView2 limit; the handoff records the measured
  1/3/5-extension costs and shared-management-view comparison.
- Work's native adapter uses the extension-free automation subprofile. Its
  isolation was checked in the lab; the separately developing Work product is
  not made available by this QA build.
- Native messaging, signed-in password-manager workflows, complete disk-residue
  removal, optional-permission UI, and the full extension catalog remain
  unqualified. The Windows catalog does not claim that every listed item works.
- Specific-sites manifest narrowing is covered by unit tests, native
  content-script/injection checks, and a native network-rule qualifier: blocking
  applies on the allowed loopback host and does not apply on the denied host.
  Reproduce the latter with `crates\zephium-webext-windows\run-host-rules.ps1`
  after building the lab feature.

## Resource sampling

Keep a fixed set of tabs, close the popup, and leave QA untouched. Use the PID
printed by the launcher:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File desktop\measure-webext-qa.ps1 -QaProcessId 1234 -Seconds 600 -Label bitwarden-idle
```

Repeat with extensions disabled for a comparable baseline. Results are written
under `target\webext-qa-resources`. The sampler includes only the QA process tree,
uses no debugger, and reports private bytes, summed working sets, process count
and CPU as a percentage of one core. Working sets can double-count shared pages;
short-lived processes can make sampled CPU an underestimate. Debug-build numbers
are qualification evidence, not release performance claims.

Detailed probe findings and remaining work: [Windows handoff](windows-extensions-handoff.md).

## Review follow-up acceptance

- Install the storage fixture from `crates/zephium-webext-windows/fixtures/storage`
  using **Install from file > Unpacked folder**. Open its popup on a normal page
  and save the test value. Change All sites to Specific sites, add that page's
  host and select Done. Reopen the popup, then switch back to All sites and
  reopen it again. The stored value should survive both changes and restart.
- Try a malformed native package. The tested invalid-CSP fixture produces
  **Couldn't start / Retry**. Retry should attempt only that install; existing
  pages and other extensions should remain usable. Remove the failed fixture
  when finished. A preparation-time rejection may instead stop at review.
- Repeat ordinary store install, disable/enable, popup opening and removal with
  Bitwarden and Dark Reader. Dark Reader's per-site popup controls still have
  the documented targeting limitation. Use a disposable account for signed-in
  Bitwarden testing; native storage preservation alone does not qualify autofill.
- Recheck profile separation and closing/reopening QA. No Work product behavior
  was added by the boundary fix. Open an extension's Options entry, then disable
  that extension and check that navigating/reloading its URL is refused. A
  different profile without the enabled install must also refuse that URL.
