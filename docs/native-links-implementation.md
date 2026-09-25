# Native links and download handoff

Implementation and local qualification, 2026-09-24. This extends the existing
macOS/Windows download brokers; it does not add local-file browsing or Linux
file workflows. Changes are in `feature/ui-design-system`, on top of `3c01d8cd`.
This record is evidence of the checks below, not release certification.

## Behavior

- Human `target="_blank"` links and user-activated `window.open` create managed
  tabs using the original native request/configuration and profile. POST bodies,
  cookies, redirects and opener relationships stay with the native engine.
- Cmd-click on macOS and Ctrl-click on Windows request background tabs; Shift
  requests foreground disposition. macOS also handles native middle-button link
  activation. Disposition does not grant popup authority.
- A foreground child is selected only after its committed document passes the
  existing chrome/presentation barrier. Newer user selection cancels that intent.
  Downloads retain the original page and dispose of their uncommitted child on
  start or cancellation, including after a space switch.
- Native image/link saves enter the existing destination, staging, history and
  quarantine/Attachment Services workflow. No separate HTTP client retries URLs.
- Native script-close requests affect only adopted children. Failed/abandoned
  adoption has an exactly-once rejection path that retires native ownership.
- Host-denied popup requests have a Rust-backed status indicator. Some WebKit
  automatic-popup denials occur before any host callback and have no indicator.

## Native boundaries

The native source must be live, presented, in the foreground window, in its
original navigation and profile generation, and within resource/admission limits.
WebKit disables automatic JavaScript windows; WebView2 requires native
`IsUserInitiated`. A per-source burst is limited to eight requests per second.
Privileged and agent views retain their existing denials.

WebKit uses the engine-supplied child configuration with a fresh script controller.
WebView2 adopts a same-environment/profile controller and reinstalls content-policy
handlers after `SetNewWindow`, before completing the deferral. Failed registration
closes the child. The first native download in a new child has a one-shot,
30-second allowance to request a destination before presentation; it does not
grant directory access or bypass save consent.

macOS context-menu downloads depend on the optional **private** WebKit selector
`_webView:contextMenuDidCreateDownload:`. Its WKDownload is passed to the existing
broker. Qualify this on every supported macOS/WebKit version, especially the
minimum OS. Native menu labels are retained, including “Download Image” and
“Open Image in New Window” (the latter creates a managed tab).

Windows native menus require an explicit bounded allowlist; document Save As,
printing, sharing and unknown commands are excluded. `SaveAsUIShowing` is also
cancelled because it is distinct from `DownloadStarting`. The engine-owned
Windows upload-dialog limitation remains as documented in `security-model.md`.

## Checks performed

Local macOS build and native QA used the isolated **Zephium Files Integration QA**
profile. Generated loopback fixtures live under `/private/tmp/zephium-links-fixtures`.

| Check | Evidence |
| --- | --- |
| Cmd-click ordinary link | User confirmed background tab and unchanged source selection |
| New-tab ordinary link | Native child document displayed; correct title after presentation |
| New-tab attachment | Native save and completed 32-byte file; transient child removed |
| New-tab POST export | Native save, source retained, exact 20-byte `POST export fixture` response; server accepts only the original POST body |
| Script-created blank child | `document.write`/`document.close` content and title displayed; source reports a returned native window |
| Delayed popup | Fixture reports `PASS: delayed popup blocked` after six seconds |
| Image context-menu download | Native save and exact 68-byte generated PNG |
| Terax website → GitHub release | User reports success; isolated QA download history showed Completed, 6.4 MB |
| Rust core/app/engine | Full library runs: 373/358/388 passed; the final app run includes the space-switch cleanup regression |
| Desktop | 106 passed; 2 existing ignored tests |
| Vendored Wry | 69 library tests passed |
| Mac strict Clippy | Core/app/engine/desktop all targets passed with `-D warnings` |
| Windows compile | Engine all-targets check for `x86_64-pc-windows-msvc` passed; three existing Wry permission dead-code warnings |
| Frontend | Check: 250 tests; WebKit component suite: 143 tests; production build and emitted stylesheet gate passed |

Saved POST SHA-256:
`e20e1ffd46648e86f29fc3ee7c697995800b07c9351b00934e6a92d865d4fd3f`.
Saved PNG SHA-256:
`c4166024f2e7da975c2c1a06b44962f891b0161538a633f8132dc4667e36153a`.
The fixture script is `scripts/qualification/file_workflows.py`; start it with
`--files /private/tmp/zephium-links-fixtures/uploads` and use the URL it prints.

## Remaining qualification

Windows native execution is pending on the user's Windows machine: ordinary and
Ctrl/Shift/middle links, target-blank redirects and POST downloads, image saves,
script-created/closed windows, popup denial, destination cancellation, tab/space
switches, private profiles, Attachment Services and Mark-of-the-Web. Cross-compiling
does not prove these behaviors. Discord signed-in attachment testing is pending.
Automated macOS middle-click input behaved like ordinary activation, so it is not
counted as runtime proof. The native modifier path exists; qualify a real middle
mouse button separately. Script-close ownership is unit tested, not yet native
UI qualified. The minimum macOS/WebKit version still needs qualification.

Full CI remains blocked by the existing inventory error in
`desktop/src/foreground_rendering_probe.rs`: external module resolves outside
scanned inventory (`desktop/foreground_probe_admission.rs`). This gate has not
been bypassed. Existing broader security/release gates remain in force.
