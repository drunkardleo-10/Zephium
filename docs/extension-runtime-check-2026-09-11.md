# Real extension runtime check — September 11, 2026

This records interactive evidence, not a release-complete claim.

The later [startup and popup follow-up](extension-startup-performance-2026-09-11.md)
records immediate saved-session projection, reduced repeated authentication work,
optimized QA timings and the Dark Reader outside-click dismissal fix.

## Environment and package

- macOS 26.6.2, isolated `Zephium Extension QA.app`, bundle identity
  `app.zephium.external-qa`.
- Actual Google Chrome Web Store Dark Reader package, version 4.9.130,
  extension ID `eimadpbcbfnmbkopoojfekhnkhdbieeh`.
- Direct store download and the ordinary browser-owned Add to Zephium action;
  no fixture catalog, backend metadata or hand-edited installation rows.
- Required permissions approved in the isolated test profile. Optional context
  menus, private browsing and local-file access were not granted.

## Observed behavior

1. The initial popup/options UI rendered correctly. Accessibility exposed hidden
   “Loading” text even after controls populated; that text was not proof of a
   stuck popup. The initial example.com page did remain light, with no Dark
   Reader styles in its DOM.
2. Inspection found Dark Reader's native content-script world, its main injected
   script and background state identifying the actual tab/document. The page
   became dark after site-toggle testing and a reload. The initial failure's
   root cause was not isolated, so no speculative background-wake patch was
   applied.
3. A clean restart without opening the popup produced a dynamic dark theme.
   The DOM had `data-darkreader-mode="dynamic"`,
   `data-darkreader-scheme="dark"`, `data-darkreader-proxy-injected="true"`,
   Dark Reader stylesheets and computed dark colors. This went beyond a visual
   fallback-style check.
4. Disabling through Zephium's manager removed the action and prevented dark
   styling on reload. Re-enabling restored an active runtime and dark styling.
5. Removal exposed a code gap: native erasure identity was resolved only through
   the old catalog. It now uses the installed external package's authenticated
   source. The actual UI subsequently completed removal, showed zero installed
   extensions, and removed the action.
6. Reinstallation from the real store completed with fresh permission review.
   Dark Reader opened its welcome page. The first subsequent example.com
   navigation was dark without popup interaction or another reload.
7. Dark Reader's own settings site toggle changed the current page to light
   without reloading. Its Option+Shift+A shortcut restored the dark theme live.
8. Navigation to `https://www.iana.org/help/example-domains` also produced a
   visibly dark page, including body, header, footer and readable links.

The final removal/reinstallation and live-toggle checks used a build without
the inspection feature. Temporary console diagnostics were discarded by app
restart. No extension script or installed sealed artifact was patched to make
the page appear dark.

## Automated checks for this change

- External-feature extension-service library tests: 193 passed.
- Default-feature boot tests: 4 passed, including the inert empty-build behavior.
- Frame typecheck: zero errors and warnings.
- Updated isolated native app bundle built and was used for removal/reinstall.

## Remaining qualification

The original first-run light-page report remains a regression scenario. The
clean reinstall succeeded, but it does not establish that every possible
initialization race is resolved. WebKit has an upstream fix for an initial
background-message race ([315975@main](https://commits.webkit.org/315975@main));
its applicability to this observed run is unconfirmed.

Offline process startup, native multiple-profile isolation, interrupted native
update recovery, sustained use, and broader extension coverage remain separate
qualification. The lifecycle follow-up below records subsequent update,
permission and storage work. Removal completed through
the native erasure protocol; this session did not independently inspect every
OS storage record after erasure. No Windows runtime result is claimed.

## Vimium direct-install follow-up

The original Chrome Web Store Vimium 2.4.2 package was rejected because the new
external installer admitted only the simpler native subset. Previous Vimium
qualification used the WebKit compatibility compiler and restricted browser
services. The store path had not yet been connected to those adaptations.

The compiler's pure planning code now lives in `zephium-extension-package`,
shared by the existing release tooling and authenticated local preparation.
Original CRX authentication precedes compilation. The local artifact records
the identity-insertion and WebKit adaptation digest, exact output tree, compiled
compatibility target and original upstream evidence. It is regenerated and
revalidated during offline reopening. The signed Beta policy and Verified
authority are not broadened. Windows retains its native path; these WebKit
adaptations are selected only for the macOS compatibility profile.

One real activation attempt exposed a second gap: the service's final backend
selection did not recognize the brokered target. That mapping is now shared
with local reopening. The failed attempt's durable state was recovered through
normal restart, without deleting or editing Store rows.

Interactive results after the fixes:

- Installed from the actual store, with explicit permission review, alongside
  Dark Reader. Both appeared Active.
- `f` displayed real link hints on IANA; typing `ad` followed the Instructions
  and Guides link.
- `o` opened the actual extension Vomnibar. Searching `example` returned the
  profile's real tab and history results. Submitting an unselected query opened
  the browser's default search engine. This was not a tab-switch verification.
- Removed Vimium through the manager and installed it again. The clean install
  activated immediately; `f` worked on an existing example.com tab without a
  browser restart or manual page reload.
- Reached that reinstall through the store logo, its search field, and the
  Vimium result. The sidebar installer remained usable through this navigation.
- Restarted the final build after the lifecycle storage/accounting cleanup.
  Both extensions restored, and Vimium link hints worked on the Dark Reader
  themed example.com page. The app was left open for continued use.

The native navigation observer now emits an exact presentation fact when the
URL changes within a committed document. Shell updates an already-presented
matching epoch without hiding or re-presenting it, and rejects stale URL facts.
This repairs a path that could leave store admission bound to the previous
listing. An initial not-ready message now tells users to let the page load and
keep the tab active instead of implying they are necessarily on the wrong page.

Additional verification: 77 distribution library tests, 194 external-feature
service tests, 14 shared compiler tests, 16 presentation tests, 6 native
navigation tests, 8 Store provenance tests, 4 default boot tests, and the
JavaScript compatibility-asset contract passed. The affected distribution and
service libraries passed Clippy with warnings denied. A narrower 35-test
admission rerun covered the final transform identity. These are focused checks,
not a fresh full-workspace CI result.

Dark Reader's own original background script explicitly excludes
`https://chromewebstore.google.com/` and `https://chrome.google.com/webstore`.
The store remaining light is therefore expected for this package.

## Update, permission and storage follow-up

The manager now offers an explicit **Check for updates** action for each local
store installation. Both real Dark Reader and Vimium checks returned **Up to
date**, with both runtimes still Active. This is an on-demand check, not an
automatic polling scheduler. It downloads from the original source and checks
the authenticated publisher, version, package content and compatibility again.
New required access opens the existing permission review; closing that review
retains the current installation and invalidates the pending approval.

Signed synthetic CRX3 packages exercised actual service and Store transactions:
unchanged packages, a newer disabled installation, new required permissions,
dismissed or stale approvals, invalid signatures, rollback, and changed content
at the same version. A changed valid CRX envelope with identical publisher,
version and ZIP content is treated as unchanged without replacing persisted
provenance. These tests do not establish a real enabled native version swap.
A post-commit native activation failure still requires the existing recovery
protocol; automatic rollback to the previous package is not claimed.

Dark Reader's optional context-menu permission was granted, survived restart,
and was revoked through the manager with both extensions installed. A later
grant used the existing additive live-grant path: the native journal incarnation
remained `23` while its grant revision advanced from `3` to `4`. Revocation then
succeeded and the final optional permission is off. Revocation retains full
runtime retirement; extension-initiated permission removal is not qualified by
this manager test. Recreating Dark Reader's context can open its help page;
the in-place additive path avoids that unnecessary recreation.

Persistent package objects now have a 512 MiB logical file-byte budget and a
65,536-entry budget. Preparation scratch retains its separate package bounds.
Collection retains references from every profile, including disabled installs,
unresolved native journals, pending reviews and live native reservations. It
reclaims at most eight objects per maintenance turn and checks the deadline
between objects. An incomplete ownership inventory refuses collection. These
bounds do not describe all OS-managed extension data or disk allocation units.

Storage tests cover disabled and shared-profile roots, pending versus dismissed
reviews, native reservations, oversized sparse orphan accounting without reading
payloads, deepest accepted package cleanup, and retry after a pre-mutation busy
path. Focused checks passed: 200 external-feature service tests, 39 distribution
Beta tests, 61 private-filesystem tests, 315 App tests and 362 Core tests. The
Store public-surface guard and UI typecheck also passed. The isolated QA bundle
was rebuilt and used for the live manager checks above.

## Broader package inspection

The real Stylus 2.4.11 package authenticates, but is outside the current
compatibility subset. The manager now names unsupported features instead of
only giving a generic refusal; no Stylus installation occurred. Its manifest
includes network interception/rules, identity, idle, offscreen documents,
side panels, unlimited storage and navigation events. Source inspection found
offscreen helpers for blobs, system appearance and other work. Some network
paths are conditional, so this is not proof that every declaration is essential
to basic styling or absent from the native engine. Admitting it safely requires
qualification and real implementations or justified adaptations.

Grammarly's separate [package inspection](extension-grammarly-check-2026-09-11.md)
found a rejected Unicode resource path and additional API/authority gaps. It is
deferred to a later compatibility phase; no Grammarly runtime support is claimed.

Automatic source updates, real enabled version-changing update/failure tests,
native profile isolation, corruption-repair UX, offline startup and sustained
use remain open. Physical Windows qualification and deployed metadata trust
remain separate. The [September 11 engine review](engine-security-review-2026-09-11.md)
refreshes the review deadline without lowering any engine version floor.
