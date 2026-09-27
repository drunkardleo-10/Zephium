# Extension Platform Roadmap

> Initial-release scope changed on September 11, 2026. Use
> [Initial extension release scope](../extension-release-scope.md) for current
> product decisions and completion criteria. The marketplace, Zephium-hosted
> package publication and per-version approval lanes below are historical or
> future scope, not prerequisites for ordinary Chrome Web Store installation.

Status: gate-driven working plan. This document is deliberately not the source
of truth for implemented security guarantees. `docs/architecture.md`,
`docs/security-model.md`, code, and named live tests override it when they
disagree. Update this plan after every material gate closes so it remains useful
for sequencing.

Checked against committed code through `455396a` plus the current reviewed-update
working slice on 2026-08-20.

## Product outcome

Zephium will be a real cross-platform extension platform, not a browser with a
few hard-coded integrations. The finished product has five distinct lanes:

1. **Native Zephium capabilities.** Performance-, privacy-, or product-critical
   features implemented in Rust/native platform code when native ownership is
   materially safer or cheaper than an extension. The ad blocker is the first
   example. Page styling, notes, tasks, reader tools, and other product features
   are evaluated by the same rubric; they are not automatically extensions.
2. **Zephium Mods.** A future community customization format for browser chrome
   and page styling. Browser-chrome mods are declarative and never receive a
   generic internal bridge. Page mods are declarative where possible; any script
   capability uses the isolated userscript boundary and explicit grants. Mods
   do not inherit WebExtension or native-product authority.
3. **Zephium Verified WebExtensions.** Exact versions selected, reviewed,
   packaged, tested, signed, staged, supported, and automatically updated by
   Zephium. Support is reported per extension version and platform. Vimium 2.4.2
   is the first macOS verification target.
4. **External compatibility mode.** User-initiated installation from supported
   Chrome, Firefox, Safari, publisher, or file sources. Every package is
   authenticated, analyzed, and admitted locally before installation. The UI
   discloses supported, degraded, and refused capabilities; external packages
   carry no blanket Zephium guarantee.
5. **Developer mode.** Explicit local unpacked/package installation with strong
   warnings, separate policy, no production update promise, and no relaxation
   of page-world or browser-internal trust boundaries.

The browser remains fully usable with no extensions. No extension worker,
repository, native controller, updater, or network client starts when product
authority and cleanup debt are both absent.

## Public experience

The primary surface is a native **Extensions Center**, not a collection of
store-specific dialogs. It has these sections:

- Verified
- Discover
- Installed
- Updates
- Compatibility
- Developer mode

Every candidate and installed row shows bounded browser-owned data only:

- publisher/upstream identity and exact version;
- source and Zephium packaging status;
- platform badges for macOS, Windows, and Linux;
- `Verified`, `Compatible`, `Degraded`, or `Unsupported` status;
- required and optional API/host authority;
- file and private-browsing access, both separately controlled;
- reviewed limitations and last verification date;
- update, rollback, quarantine, and restart-required state;
- locally measured resource diagnostics when available.

The installation flow is:

```text
candidate -> compatibility/permission review -> authenticated immutable bytes
          -> local reauthentication -> atomic install -> runtime activation
```

Updates repeat the complete acquisition and compatibility pipeline. New
required permissions, new degradations, publisher/source changes, or unsupported
declarations pause activation and require an explicit decision. A failed update
never replaces the last known-good generation.

External-store installation is behind an explicit product setting until its
legal, acquisition, update, and compatibility gates pass. Zephium must not
claim that an extension is supported merely because its manifest parses or its
popup opens.

## Native feature versus extension versus mod

Use the following decision rule before adding a popular extension to the
Verified catalog:

Choose a **native feature** when most of these are true:

- it is central to the browser's identity or default privacy posture;
- it belongs in a navigation/network/rendering hot path;
- it can be implemented with materially lower steady-state CPU/RAM/wakeups;
- it needs browser-internal state that must not be exposed through a bridge;
- consistent cross-platform behavior is more valuable than upstream parity;
- Zephium is willing to own long-term product UX and maintenance.

Choose a **Verified WebExtension** when most of these are true:

- users expect the upstream product and its account/service ecosystem;
- replacing it natively would fragment user data or security expectations;
- the publisher already maintains the feature as a WebExtension;
- compatibility can be expressed through explicit API/grant contracts;
- keeping upstream update velocity is valuable.

Choose a **Zephium Mod** when the capability is customization rather than
general browser authority: theme tokens, typography, declarative page styles,
element rules, browser layout options, commands, or isolated user scripts.

Examples:

- ad/tracker blocking: native;
- Arc-style page appearance and typography: native engine plus declarative mods;
- notes/tasks: native product domains;
- Vimium: Verified extension;
- password managers: Verified extension or publisher partnership, not a partial
  native clone;
- Dark Reader: support the extension where practical, while a separate native
  page-appearance feature may cover common darkening/customization without
  pretending to be Dark Reader.

Do not copy algorithms or assets from an extension into native code without a
separate license, provenance, and maintenance review.

## Platform strategy

All three desktop platforms remain product requirements, but delivery is
sequential so each runtime is proven before its policy is copied elsewhere.

| Order | Platform | Runtime strategy | Current claim |
|---|---|---|---|
| 1 | macOS 15.4+ | `WKWebExtensionController` plus narrow Zephium brokers | Active productionization target |
| 2 | Windows | WebView2 native extension runtime plus Zephium-owned UX | Foundation/spikes only |
| 3 | Linux | Zephium MV3 compatibility runtime on WebKitGTK | Required later; major runtime project |
| Later | older macOS | Reuse compatible portions of the Linux runtime if justified | No current claim |

Shared package, catalog, permission, management, profile, update, and UI domains
must remain platform-neutral. Native controller/view/process logic remains
inside platform adapters. Linux difficulty is not permission to pollute the
shared architecture or lower macOS/Windows guarantees.

## Support tiers

### Native

Browser capability owned and supported as part of Zephium itself. It is not
listed as an installed extension and cannot be overridden by extension update
metadata.

### Verified

An exact `(extension, version, package, platform, runtime target)` tuple passed:

- origin and publisher/provenance review;
- license and redistribution review;
- manifest/API/permission classification;
- deterministic package preparation and signature verification;
- content/background/action/popup/options workflows as applicable;
- permission, private-mode, file-access, profile-isolation, update, rollback,
  uninstall, restart, and profile-deletion gates;
- hostile page/cross-extension tests;
- release-build resource and endurance budgets.

### Compatible

No unsupported/unassessed authority was admitted, but the exact extension has
not passed the complete Verified workflow. Any reviewed degradation is visible
before install and in management UI.

### Degraded

The package can run safely but one or more named features cannot behave like the
upstream browser version. Degradation is never inferred from package name and
never hidden behind a generic warning.

### Unsupported

Unknown authority, unsafe semantics, an unavailable security boundary, or a
required capability that cannot be implemented safely prevents activation.
Unsupported packages may be analyzed and displayed; they do not receive a live
runtime.

## Security and resource invariants

These decisions are not retrofit-safe:

1. Every installed extension is a distinct non-reusable principal.
2. Extension UI is unprivileged and receives no Tauri/Wry application bridge.
3. Raw page worlds never receive an extension/native bridge.
4. Native identity is bound by native registration and never asserted by
   JavaScript.
5. Required and optional API, host, file, and private-mode grants are separate.
6. Unknown or unassessed declarations fail closed.
7. Adaptation is offline, package-neutral, versioned, deterministic, and
   non-authorizing until signed catalog admission.
8. Exact compatibility receipts are content-addressed inside signed release
   trees and bound by catalog/inventory authority.
9. Extension DNR generations remain separate from the native blocker.
10. Extension activity cannot resurrect discarded tabs around the browser's
    native-view pressure policy.
11. Zephium-created views, reservations, and cleanup debt stay inside
    `MAX_NATIVE_VIEW_RESOURCES = 48`; platform-managed extension processes get
    separate measured budgets.
12. No always-live background process per extension. MV3 event/runtime liveness
    follows explicit pending-event, request, port, and idle state.
13. No cloud connection is required to execute an already installed extension.
14. Catalog, package, permission, and native lifecycle failures are typed,
    bounded, redacted, and recoverable or explicitly restart-required.

High-risk categories require independent programs, not casual compatibility
claims: password managers, wallets, arbitrary native messaging, proxy/VPN
control, enterprise policy, debugger/devtools authority, capture APIs, and
financial signing. They are outside the first catalog, not declared impossible
forever.

Blocking `webRequest` is not available on the current macOS native runtime.
Registration acceptance is not blocking enforcement. Products depending on it
must be classified honestly or use a separately reviewed brokered design.

## Current state

### Closed foundations

- inert zero-extension startup;
- bounded duplicate-key-safe manifest/catalog/tree parsing;
- CRX3 authentication and deterministic externally signed release preparation;
- package-neutral compatibility artifacts and embedded receipts;
- fixed-origin distribution client and dormant updater worker;
- immutable repository, staged candidate/current/previous catalogs, rollback,
  crash recovery, cleanup, bounded GC, and live leases;
- per-profile install/grant catalogs and native ownership journal;
- macOS controller/data-store binding and live profile/private isolation;
- native activation, recovery, retirement, namespace erasure, and clean restart;
- Shell-owned logical window/tab projection with discarded-tab residency
  protection;
- content script/background messaging and on-demand background wake;
- tabs query/create/activate/update/navigate/remove;
- action toolbar ownership and transient popup presentation;
- install review, optional grants, enable/disable/uninstall, update status, and
  browser-owned management projections;
- bounded recent-history compatibility broker;
- real authenticated MV3 product probe;
- stock Vimium 2.4.2 native/brokered workflows;
- stock Dark Reader 4.9.129 native workflows in the mixed staging catalog;
- acquired brokered package install/management/restart E2E;
- preliminary release-build macOS product-path measurement plus a
  LaunchServices-coalition process-family sampler;
- exact changed-required-authority/new-degradation update review and atomic
  approval transaction;
- exact per-extension uninstall erasure with persistent zero readback on both
  native and brokered macOS product paths;
- full `cargo xtask ci` coverage for the present tree.

### Open macOS product gates

- ordinary production authority and catalog remain deliberately empty;
- the opt-in, separately identified macOS staging application embeds an exact
  revision-8 Vimium 2.4.2 plus Dark Reader 4.9.129 catalog; ordinary builds
  remain inert;
- the native Extensions Center supports installed/verified discovery, review,
  install, enable/disable, options, uninstall, update status, and changed-
  authority/new-degradation update review;
- no packaged-app end-to-end run starts at discovery and ends at profile
  deletion using a real Verified package;
- production signing/KMS, fixed origins, publisher/legal approvals, staged
  rollout, revocation, and support operations are not provisioned;
- the staging Verified catalog has two proven targets rather than a release
  representative cohort;
- password-manager core workflows remain unresolved on current WebKit;
- macOS local-file execution remains unavailable: the dedicated dynamic-install
  gate creates three post-WebView contexts whose exact file grants are accepted
  and read back, but whose isolated content script never executes in a real
  local document during the bounded settle window. Do not expose the file
  toggle until a future WebKit change deliberately flips that negative gate;
- external Chrome/Firefox/Safari/file acquisition is not a product feature;
- cross-process helper RSS, wakeups, battery, tab-scale pressure, and 24-hour
  endurance do not yet have release budgets;
- packaged hostile-page, cross-extension, update-failure, and crash-loop gates
  remain open.

## Roadmap to finished macOS support

### M0 — Product contract and plan

- [x] Adopt native / Mods / Verified / Compatibility / Developer lanes.
- [x] Keep Linux required but sequence it after macOS and Windows.
- [x] Encode the Verified / external compatibility / developer-local source lane
  in typed core/IPC management projections so UI cannot infer it from names.
- [x] Add bounded reviewed publisher/upstream provenance and verification date
  to the product catalog and management projection without exposing raw paths
  or mutable remote metadata.
- [ ] Document which native features intentionally replace common extension
  categories and which upstream extensions remain supported.

### M1 — Extensions Center browser UX

- [x] Replace the compact manager's product role with a scalable native
  Extensions Center while retaining the toolbar button as a quick entry point.
- [x] Separate the currently actionable Installed and Verified sections with
  responsive modal layout, keyboard tab semantics, and first-run discovery.
- [x] Expand the existing privileged chrome surface for the Center's exact
  visible lifetime, hiding native page siblings and restoring their layout on
  close without creating another persistent privileged WebView.
- [ ] Add dedicated Updates, Compatibility, and Developer sections when their
  corresponding browser capabilities become actionable.
- [x] Show source, publisher, exact version, support tier, verification
  date, permissions, private/file access, limitations, and update state.
- [ ] Preserve complete stale-resistant selectors and actor-ordered projections;
  UI receives no raw paths, native errors, or authority-bearing objects.
- [ ] Add local safe mode and per-site extension disable controls without
  unloading unrelated profiles.
- [x] Make unavailable ordinary builds silent rather than presenting an empty
  successful store.

### M2 — Local/staging Verified Vimium flow

- [x] Build a provider-neutral catalog publication tool from reviewed release
  artifacts: canonical catalog, legal object, CRX3 target, manifest profiles,
  product anchors, and deterministic evidence.
- [x] Create a non-shipping staging authority and fixed local/staging transport;
  never read endpoints, package keys, or runtime targets from page content,
  preferences, command line, or ambient environment.
- [x] Provision Vimium 2.4.2 as the first optional Verified extension.
- [x] Exercise discover -> review -> install -> use -> options/popup -> update ->
  disable -> enable -> uninstall -> restart -> profile delete.
- [x] Prove no package-specific runtime logic is introduced outside typed
  compatibility/catalog data and executable verification contracts.

### M3 — Packaged macOS release readiness

- [ ] Run the M2 flow from signed/notarized packaged application builds on every
  supported macOS line and architecture.
- [ ] Prove profile/private isolation, extension storage/cookies, discarded tabs,
  popup pressure, controller teardown, namespace erasure, restart recovery, and
  update rollback in the packaged app.
- [ ] Add hostile package/page/extension tests, crash loops, interrupted updates,
  corrupt local state, offline startup, CDN failure, and revocation behavior.
- [x] Require explicit, exact-replacement review for newly required API/host
  authority and newly introduced compatibility degradations while leaving the
  old runtime live; commit approved package plus required grants atomically.
- [ ] Measure browser plus WebKit helper process families: clean startup,
  activation, one/three/maximum admitted extensions, idle CPU/wakeups, RSS,
  energy, tab pressure, popup churn, and 24-hour endurance.
- [ ] Set release budgets from clean dedicated runners; do not turn current
  observations into limits retroactively.
- [ ] Update `docs/security-model.md` deliberately before making a public claim.

### M4 — Representative Verified catalog

- [x] Verify a simple control extension (Vimium 2.4.2).
- [x] Verify Dark Reader 4.9.129 as a demanding style/content workload while
  separately designing Zephium's native page-appearance feature.
- [ ] Verify an authenticated cloud/service extension.
- [ ] Pursue a publisher-supported password-manager path (1Password is the
  preferred partnership target); retain Bitwarden/Proton evidence as platform
  diagnostics rather than product-name hacks.
- [ ] Define update cadence, supported-version window, incident ownership, and
  end-of-support behavior per extension.

### M5 — External compatibility mode

- [ ] Define source-specific, legally reviewed acquisition adapters for Chrome
  Web Store, Firefox AMO, Safari/publisher packages, CRX/XPI, and unpacked trees.
  Do not scrape or bypass store interfaces/terms.
- [ ] Recognize canonical store URLs/IDs and present the Zephium install review;
  never inject a privileged generic downloader into page content.
- [ ] Run static compatibility/security analysis before download and again after
  complete authenticated materialization.
- [ ] Present per-platform supported/degraded/unsupported results before install.
- [ ] Re-run classification on updates and stop on permission, publisher,
  source, or capability drift.
- [ ] Add redacted local diagnostics/console and a support bundle with no secrets,
  browsing history, tokens, or arbitrary page data.
- [ ] Keep external installation separately disableable by users and enterprise
  policy.

### M6 — Zephium Mods and native page appearance (parallel after M2)

- [ ] Specify separate declarative schemas for browser chrome and page styling.
- [ ] Keep browser-chrome mods token/layout/command based; no arbitrary JS or
  private DOM/IPC access.
- [ ] Reuse isolated userscript principals for scripted page mods with explicit
  match/grant policy and hard source/runtime budgets.
- [ ] Build native page appearance controls for color scheme, contrast,
  typography, font substitution, spacing, and reviewed element rules.
- [ ] Measure style recalculation, navigation cost, memory, and site breakage.
- [ ] Keep this track independent from WebExtension compatibility and do not use
  it to fake unsupported extension APIs.

## Infrastructure/control plane (built later, designed now)

The browser owns interfaces and authenticated state now; production services are
implemented after the local/staging macOS flow proves the product contract.

Required services:

1. **Catalog Authority.** Canonical signed metadata, monotonic revisions,
   candidate/current/previous, platform matrices, revocations, and transparency
   evidence.
2. **Package Ingestion.** Source-specific fetchers, publisher identities,
   license/provenance capture, malware/static analysis, and immutable originals.
3. **Compatibility Build.** Deterministic package-neutral transforms, SBOM,
   corresponding source, receipts, and reproducibility checks.
4. **Test Farm.** Real supported macOS/Windows/Linux runners executing extension
   workflow contracts, hostile cases, performance, and endurance.
5. **Signing Service.** Dedicated P-256 extension identity in KMS/HSM; GitHub OIDC
   or equivalent short-lived workload identity; private keys never enter source,
   artifacts, logs, or general CI secrets.
6. **Object Storage/CDN.** One fixed HTTPS origin, immutable content-addressed
   targets, exact lengths, no redirects/compression ambiguity, controlled
   publication order, and rollback retention.
7. **Rollout Coordinator.** Internal -> canary -> percentage -> stable promotion,
   health gates, quarantine, rollback, and emergency revocation.
8. **Publisher Portal/API.** Ownership verification, submissions, compatibility
   results, release approval, support contacts, and partnership packages.
9. **Operations.** Audit logs, least privilege, separation of signing/publishing,
   backups, incident playbooks, key-compromise migration, availability/error
   budgets, and privacy-preserving opt-in diagnostics.

Recommended origin layout:

```text
https://extensions.zephium.com/stable/metadata/
https://extensions.zephium.com/stable/targets/
```

The provider is intentionally undecided. The browser depends on fixed-origin
protocol contracts, not AWS/GCP/Azure/Cloudflare SDKs.

## Windows phase

Begin only after the macOS Verified flow is user-testable and its shared
contracts are stable.

- [ ] Replace Wry's result-discarding `AddBrowserExtension` helper with exact
  asynchronous settlement and retained extension identity.
- [ ] Enable native extension support at WebView2 environment creation without
  imposing startup cost when no authority/install/cleanup debt exists.
- [ ] Bind profile, regular/private context, immutable package directory,
  install, and native extension identity.
- [ ] Implement Zephium-owned action toolbar, popup/options surfaces, management,
  permission prompts, updates, rollback, uninstall, restart, and profile delete.
- [ ] Execute the same Verified contracts and report platform-specific
  limitations rather than pretending the macOS matrix transfers.
- [ ] Finish the separate CDP userscript spike only for userscript/Mod needs; it
  is not a page-world fallback for WebExtensions.
- [ ] Set Windows process-family memory/CPU/endurance budgets.

## Linux phase

Linux support is required, but WebKitGTK does not provide a complete extension
runtime. Treat this as its own runtime project:

- [ ] Reuse existing match-pattern, userscript, protected-script, per-principal
  world, and handler foundations.
- [ ] Implement an MV3 event/background state machine within explicit native-view
  and memory budgets.
- [ ] Implement capability-scoped browser APIs through shared Shell/service
  domains; never add a generic fetch/native bridge.
- [ ] Support content scripts, messaging, storage, tabs/windows, action/popup,
  commands, permissions, web-accessible resources, and DNR in reviewed slices.
- [ ] Disclose Linux frame-targeting ceilings unless a separately secured
  WebProcess extension is justified.
- [ ] Run the same package, profile, lifecycle, hostile, update, and resource
  gates on supported distributions/WebKitGTK floors.

## Definition of finished macOS extension support

macOS support is not finished until all are true:

- ordinary release builds contain a valid production authority or remain
  provably inert;
- the Extensions Center supports the complete Verified lifecycle;
- at least one representative catalog cohort, including Vimium and a
  security-sensitive publisher-supported extension, passes packaged workflows;
- external compatibility mode is either shipped with its full contract or
  explicitly deferred without affecting the Verified promise;
- every admitted permission/API is supported or visibly degraded;
- updates, rollback, revocation, offline startup, corruption, crash recovery,
  uninstall, restart, and profile deletion are proven;
- hostile pages/extensions cannot reach privileged bridges, other profiles, or
  other extension principals;
- native safety scripts and browser security policies survive extension
  lifecycle mutation;
- dedicated release runners meet measured startup/RSS/CPU/wakeup/energy/tab-scale
  and endurance budgets;
- signing, provenance, legal, support, incident, and release operations are
  owned;
- `cargo xtask ci` and packaged native gates are green on the supported matrix;
- public documentation states the exact platform/version/support contract and
  does not claim arbitrary Chrome/Firefox parity.

## Verification discipline

Every code slice runs focused tests and then:

```text
cargo xtask ci
```

CI success proves only the automated source/native fixtures it actually runs.
Packaged real-machine, release-origin, signing, performance, endurance, legal,
publisher, and incident gates remain distinct and must be recorded separately.

No temporary upstream checkouts, unpacked store packages, private keys, local
release artifacts, or generated production catalogs are committed accidentally.
Production configuration and legal/source artifacts are committed only through
their explicit reviewed release process.
