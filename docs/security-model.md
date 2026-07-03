# Zephium Security Model

Zephium renders hostile, attacker-controlled code by design (the web). This document
defines the trust boundaries, the invariants that must always hold, and the per-PR
checklist. The codebase and review (human and automated) are held against it.

## Trust boundaries

Three zones, ordered by privilege:

1. **Rust core (trusted).** Owns all authoritative state, persistence, and native APIs.
   Full OS access.
2. **Our UI webviews (privileged, semi-trusted).** The Solid UI: the chrome webview,
   overlay surfaces (launcher, palette, find, floating panels; raw wry with our typed
   bridge), and internal pages (history, settings, notes, easel, data viz) hosted in
   the content stage. All hold a command bridge, load ONLY bundled local assets, never
   remote content. An XSS in any of them can call exposed commands, so the command
   surface is their blast radius. The overlay bridge is held to the same standard as
   Tauri IPC: typed, allowlisted, validated in Rust.
3. **Tab content webviews (untrusted, hostile).** Raw Wry child webviews rendering the
   web. No IPC bridge, no privileged API, no access to internal protocols. Isolated by
   the engine sandbox. Code WE inject into them (scrollbar CSS, adblock cosmetics,
   Boosts, userscripts, future extension content scripts) executes in this zone and
   gains no privilege from being ours.

The wall between zone 3 and zones 1-2 is structural: content webviews are never given a
bridge. This is the single most important property. The wall between zone 2 and zone 1
is the IPC allowlist plus input validation.

Two boundary rules that follow:

- **Stage rule.** A stage pane hosts either a zone-3 content view or a zone-2 internal
  view, never a mix. Internal views are navigation-locked to bundled assets by the same
  handler that locks the chrome.
- **Profiles are a security boundary.** Each profile owns its own webview data
  partition (cookies/storage) and its own database. Incognito is a non-persistent
  partition plus an in-memory store; nothing touches disk.

## Inherited defenses (do not reimplement, do not undermine)

From the engine (Chromium/WebView2, WebKit/WKWebView, WebKitGTK):

- Process sandbox and site isolation; a renderer cannot touch the OS directly.
- Same-Origin Policy, CORS, TLS and certificate validation, HSTS, mixed-content
  blocking, cookie attributes and partitioning, CSP enforcement, sandboxed iframes,
  COOP/COEP.
- Engine memory-safety patches via runtime updates.

From Tauri (chrome only): IPC allowlist via capabilities, no Node runtime, CSP injection
for chrome assets, optional Isolation Pattern.

We must never weaken these: no "disable web security" flags, no universal file access,
no SOP-bypassing custom protocol reachable by content.

## Invariants (must always hold)

1. Tab content webviews are built without an IPC handler and without privileged
   initialization scripts. `window.__TAURI__` and any command bridge are undefined in
   page context.
2. The chrome webview never navigates to or loads remote/untrusted content. Its origin
   is locked to bundled local assets.
3. Page-derived data (title, URL, favicon, document content) is untrusted. It is never
   inserted into chrome DOM as HTML, never used to build executable or command strings,
   never used unescaped.
4. Every navigation passes the scheme policy before it commits. Disallowed schemes are
   blocked.
5. Every IPC/bridge command validates its inputs in Rust. No zone-2 surface is a
   trusted input source; messages from injected zone-3 code even less so.
6. Internal/custom protocols (asset, tauri, ipc, and any Zephium scheme) are unreachable
   from tab content.
7. Permissions (geolocation, camera, mic, clipboard-read, notifications) default to deny
   and require an explicit per-origin decision.
8. Downloaded files receive mark-of-the-web (Windows Zone.Identifier, macOS
   com.apple.quarantine) and are never auto-executed.
9. TLS/certificate errors stop navigation with an interstitial; there is no silent
   bypass.
10. Auto-updates are signature-verified before applying.

## Controls by area

**Chrome/content wall.** Content = raw Wry, zero bridge. Chrome = local assets only,
navigation locked to its own origin. Page-derived strings render as text only (Solid
escapes by default; never `innerHTML` with such data).

**Navigation and scheme policy (`navigation/`).** Allowlist: http, https, about:blank.
Block: file, javascript, internal schemes, smb, and similar; data only in tightly-scoped
cases. External schemes (mailto, tel, app protocols) are gated behind explicit user
confirmation, never auto-forwarded to the OS shell. `window.open`/`_blank`/new-window is
intercepted and routed into the tab model under the same policy. Enforced both in the Wry
navigation handler and in the `navigate` command (defense in depth).

**IPC surface (`ipc/`).** Minimal commands. Every command validates and clamps inputs;
URL arguments are scheme-checked. No command takes a raw filesystem path from the chrome
and acts on it without validation. Capabilities restrict the command set to the chrome
window.

**Injection pipeline (scrollbar CSS, cosmetics, Boosts, userscripts).** One mechanism,
one policy. Injected code runs in zone 3 (isolated worlds where the engine provides
them) and never carries a privileged bridge. Messages arriving from injected code over
the messaging channel are page-reachable in principle and are treated as zone-3 input:
validated, never trusted to name tabs/files/commands directly. Boosts and userscripts
are user-authored, per-origin, opt-in; they can alter pages (that is their job) but can
never reach zones 1-2.

**Extensions (tiered, see architecture.md §10).** Extensions are third-party code and
get their own trust treatment, distinct from both zones 2 and 3:

- Extension content scripts run in zone 3 isolated worlds.
- Extension background contexts run in dedicated surfaces that expose ONLY the
  extension API we mediate, never our command bridge.
- Every extension API call is mediated and validated in Rust against the permissions
  declared in the extension's manifest and granted by the user at install time.
- On Windows Tier 1 (WebView2 `AddBrowserExtension`), Chromium's own extension model
  applies inside the engine; our surface is install consent, permission display, and
  hosting popups in overlay surfaces (which must NOT leak our bridge to
  chrome-extension:// content).
- Extension-derived strings (names, descriptions, messages) are untrusted for UI
  rendering, same rule as page-derived data.

**Permissions.** Deny-by-default broker, per-origin prompts, persisted decisions. Wired
to WebView2 `PermissionRequested` and WKWebView delegate methods.

**Downloads (`downloads/`).** Driven by webview download events, owned in Rust. Confirm
destination, set mark-of-the-web, show the true file extension, gate dangerous types,
never auto-run.

**TLS/certificates.** Surface engine cert errors, show an interstitial, default to not
proceeding.

**Storage and privacy (`storage/`).** Zero telemetry by design. Controlled persistent
store with clear/erase. Incognito = ephemeral non-persistent data store. Profiles =
separate stores, not simulated.

**Updates.** Signed updates (Tauri updater with signature verification). Require a
minimum engine-runtime version where relevant.

**Anti-spoofing.** Honest origin display, punycode for mixed-script domains, honest
security state. Fullscreen transitions show an indicator. Content cannot draw over the
chrome (separate webview).

## Threat model

In scope: hostile web content attempting to reach privileged APIs, exfiltrate local
data, escalate via downloads or protocol handlers, spoof UI, or breach the chrome/content
wall; minimizing the blast radius of a compromised (XSS'd) chrome.

Out of scope (inherited or not our layer): engine sandbox escapes (engine vendor's
domain, mitigated by keeping the runtime updated), physical-access attacks, OS-level
compromise, supply-chain of upstream crates (mitigated by review, lockfile, minimal
dependencies).

## Platform notes

- macOS WKWebView has NO Safe Browsing (Safari-only). Anti-phishing must be added
  separately on macOS. Windows WebView2 can enable SmartScreen.
- Mark-of-the-web differs: Windows = Zone.Identifier ADS, macOS = com.apple.quarantine
  xattr. Behind the platform trait.

## Per-PR security checklist

- [ ] No new IPC/bridge command without input validation in Rust.
- [ ] No page-derived string rendered as HTML or used to build executable/command strings.
- [ ] No new capability or plugin exposed to zone-2 surfaces without justification (it
      is attack surface).
- [ ] Any new navigation path goes through the scheme policy.
- [ ] No content-reachable custom protocol added.
- [ ] No injected script or extension surface gains a privileged bridge; injection
      scope stays per-origin/per-profile as declared.
- [ ] New zone-2 surfaces (overlays, internal pages) are navigation-locked to bundled
      assets.
- [ ] Downloads (if touched) set mark-of-the-web and do not auto-execute.
- [ ] No web-security flag disabled; zone-2 surfaces remain local-only.
- [ ] No secrets/keys committed; no telemetry added.
