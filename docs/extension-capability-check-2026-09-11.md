# Local capability integration — September 11, 2026

This follow-up connects reusable WebKit adaptations to direct store installation.
It is not a claim of complete 1Password or general Chrome extension parity.

## Implementation

Local selection now considers declared API requirements, module backgrounds and
WASM CSP. Existing simple-native and Vimium brokered artifacts retain their
profiles and transform digests. The new local adapted profile reuses the existing
compiler: module workers become nonpersistent extension background documents;
classic workers remain workers. Privacy, notification, navigation and extension
page adaptations are selected from declarations, without extension-name branches.

Local native messaging separately binds the complete authenticated publisher key
to an inspected native host, Chromium principal and macOS signing identity.
1Password is the first registration. This is not a version approval list. The
compiled transformation digest and persisted evidence include the binding;
offline reopening rederives it, and repository delegation carries it to the
existing grant-checked, signature-verifying native process broker. Verified and
signed-policy Beta authority are unchanged. Windows does not inherit a WebKit
adaptation or a macOS native-host approval.

Unsupported downloads, idle detection, extension management and HTTP-auth fill
are explicitly disclosed as unavailable. Admission with those limitations does
not implement the corresponding API or establish that every extension tolerates
its absence. Unknown native publishers, identity, side panels, offscreen APIs,
unlimited storage and other unimplemented declarations still refuse admission.

## Real package and native evidence

- Original Google CRX: 1Password 8.12.37.1, ID
  `aeblfdkhhhdcdjpifhhbdiojplfjncoa`.
- CRX SHA-256:
  `0d82b1abbae90373db4055e424797a294dccbcf2cabb887aa5c240a5a9316fa6`.
- Developer-key SHA-256:
  `041b53a7773239f8577138e9fb59d2e02599f358bd24094b9e875402b0cc7927`.
- Original archive: 999 files, 45,406,559 expanded bytes. Prepared output:
  1,006 files. Authentication, preparation and offline reconstruction passed.
- The installed helper's actual signing identity is
  `com.1password.browser-support`, team `2BUA8C4S2C`; its Chrome registration
  authorizes the original extension ID.
- The isolated QA bundle was locally signed with the existing Developer ID
  identity. No release or package publication occurred.
- Initial unoptimized-debug store preparation timed out. A two-second stack
  sample identified hashing/decompression and durable file writes during
  repository materialization. Optimizing sha2/miniz_oxide/flate2 in the dev
  profile retained all integrity checks and the same deadline; the rebuilt
  store flow reached consent and successfully installed the package.
- Manager showed Dark Reader, Vimium and 1Password Active. The first two restored
  from their existing sealed artifacts. 1Password's popup remained loading.
- A subsequent diagnostic build confirmed Document background plus the expected
  publisher-host binding, actual native port callbacks, helper startup and
  response, followed by helper exit. Thus this is not a missing broker callback.
- The test account's 1Password Browser settings had connection enabled but an
  empty additional-browser list. After the user added browser trust, they
  reported that the extension worked fully without observed issues and continued
  working after fully closing the desktop app. This is user-observed workflow
  evidence. The report did not enumerate individual fill/save/passkey scenarios,
  so those are not separately certified here. First-time browser-only enrollment
  in a fresh profile remains distinct from continued operation after desktop
  enrollment and trust establishment.

## Automated checks and remaining work

84 distribution tests, 200 external-feature service tests, three Core provenance
tests, three permission-label tests, the native-host identity-table test and
external service Clippy passed. A synthetic package with an unrelated signing
key selects the reusable adapters and reopens offline without native-host
authority. A package merely naming itself 1Password cannot acquire that authority.

Record explicit synthetic fill/save, cold browser restart and removal tests.
Standalone browser-only initialization is separately unqualified; desktop
integration must not be described as a universal 1Password requirement. General
coverage, native resource qualification and Windows verification remain open.
The follow-up below replaces the original three-runtime ceiling.

Reference: [1Password additional-browser setup](https://support.1password.com/additional-browsers/).

## Setup guidance and runtime capacity follow-up

The ordinary install review now includes a short 1Password desktop-connection
note and a button opening the vendor's live setup instructions. The matching
[user guide](extensions-user-guide.md) documents setup, manual updates and
limitations. No background loading detector, automatic trust change or hosted
documentation endpoint was introduced.

Shared admission now allows twelve runtime owners across three distinct active
profiles. Planning and retiring owners retain both capacities until settlement.
The engine's total native resource bound, service drain/restore arrays and
startup readiness evidence use the same runtime limit. In particular, startup
no longer incorrectly compares extension count with the profile count.

201 external-feature service tests, eighteen acquired coordinator E2E tests,
thirteen engine resource tests, service Clippy and the full frontend check
(including 77 tests) passed. Tests retain the
fourth-profile refusal and exercise pending extension-slot exhaustion without
eviction. Physical Windows and twelve-extension resource/endurance qualification
are not implied by these checks.

The real JSON Formatter 0.10.2 package installed through the store flow with
Dark Reader, Vimium and 1Password already active. The manager showed all four
Active. On `https://httpbin.org/json`, JSON Formatter rendered the syntax-colored
tree; its Raw and Parsed buttons switched the actual document presentation.
This used the existing capability path with no JSON Formatter-specific code.
After a full browser quit and restart, the manager again showed all four Active.
Its original CRX SHA-256 is
`5e0ebde71ee60f997a9f5fc0850e6ae4a9f67006dbf662f00507f99e957c2b21`.

Cookie-Editor 1.13.0 was inspected as another candidate. Its manifest declares
`side_panel`, `sidePanel` and `devtools_page` in addition to cookies, storage and
tabs. Those UI integrations remain unsupported; the cookie permission alone is
not its complete runtime contract. No Cookie-Editor installation is claimed.

## September 12 cross-browser packaging

Local preparation now handles an exact shared MV3 background entrypoint declared
through both `scripts` and `service_worker`. The source carries a distinct
background contract; output selects the native environment, with versioned
transformation evidence and unchanged original CRX retention. Signed Beta does
not inherit this source admission. Firefox-only metadata and HTTPS editor schema
references are inert; they never supply publisher identity or network authority.

Adapted extension pages may use an implicit HTML head for the bounded supported
leading-tag forms. Only those pages receive a versioned normalization before the
existing prelude compiler. Existing explicit-head inputs and recipe hashes remain
unchanged. No page-time observer, polling task or additional background runtime
was introduced.

Refined GitHub 26.9.12 (`ea753dd699def93ff56d884bf76727f9a25b5e53b217f3ebdecfa25ae277117a`)
now passes original-source preparation and offline reconstruction. Simple Translate
3.1.0 also passes the existing module-background path. Stylus 2.4.11 still needs
required offscreen/sign-in/side-panel support; Augmented Steam 4.8.3 requires
offscreen documents; LanguageTool 11.2.3 still needs its managed-storage contract.
None of those remaining requirements were removed to force installation.

Reference: [Mozilla cross-browser background declarations](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/background).

Live release QA: Refined GitHub installed via the Store sidebar, opened its native
welcome page, and showed Active. Its normalized options document initialized the
feature list (223 features); the public repository showed file-age highlighting.
No GitHub token or sign-in was added. After a full quit/restart, Refined GitHub,
Dark Reader, Vimium and 1Password all showed Active. Simple Translate's translation
workflow remains untested. Windows output/reopen tests passed, but physical
Windows runtime qualification remains separate. Validation: 110 package, 91
distribution, 213 service and two compatibility-identity tests; desktop Clippy,
release packaging and strict local signature verification passed.

## September 19 storage composition and native action correction

Local admission retains the historical recipe when it succeeds. Only rejected
packages retry with combined bounded-storage and native adapters; their composed
recipe digest/revision binds both transformations. Module-background and history
adapter combinations pass original-source preparation and offline reconstruction.
Signed Beta admission does not inherit the expansion.

Managed-schema packages on macOS now use a versioned v2 read-only fallback when
native managed storage is absent. It returns no administrator policies, honors
caller-supplied JSON defaults without storing them, rejects writes, and retains
bounded listener/reconciliation state. Schemas remain authenticated package
resources; no schema URL is fetched or schema default promoted to policy. The
existing private managed bridge path contains v2 bytes only in these new artifacts;
older v1 recipes remain unchanged. Isolated content scripts and extension pages
receive the adapter; MAIN-world scripts do not. New managed limitations trigger
update review even when no additional API permission is requested.

Live testing found the native toolbar path invoking a dormant host-document
activeTab broker with no production owners. That redundant call rejected actions
requesting activeTab. Native toolbar dispatch now relies on WKWebExtension's
performActionForTab user-gesture handling after the existing exact owner, tab,
revision, permission and popup admission checks; no synthetic broker owner is
created. Host-document broker authority remains closed.

LanguageTool 11.4.0 original CRX SHA-256:
`fc030bc22930cd85400309f18a8e3824ba9e64f8025739b24379d3077633f5d6`.
Preparation/reopen, Store installation, native popup after restart, and
Disabled -> Active transitions passed. Cloud grammar suggestions remain unverified:
the vendor's first-run screen offers a Premium trial, and automatic approval review
blocked "Start using LanguageTool" pending user authorization. No credentials or
payment details were supplied.

For isolated public-data tests, an external-extensions-qa build accepts
`ZEPHIUM_EXTERNAL_QA_ISOLATED=1`. This selects the fixed
`isolated-compatibility-v1` child of QA app data, with a fresh generated profile and
separate native data. Ordinary launches retain their existing data root; shipping
builds do not include this switch. Quit the existing QA process before switching.

Validation: 94 distribution tests, 214 service tests, 90 engine authority/lifecycle
tests, nine popup tests, 79 frontend tests and the QA data-root test passed. The
managed-v2 JS behavior checks run with the existing compatibility asset contract
suite. Desktop Clippy and release packaging/signature verification passed. Windows
runtime managed-storage qualification and offscreen support remain outstanding.

Follow-up: the user authorized the LanguageTool onboarding/trial step, and
"Start using LanguageTool" completed to the vendor's installation-finished page.
After reloading the independent httpbin form and entering synthetic grammatical
errors, no inline suggestions were observed. Cloud/page-level grammar checking
is therefore still unqualified; the approval block is resolved, but installation
and popup success must not be presented as a completed writing workflow.

The user subsequently tested LanguageTool and reported that it works great.
This adds user-observed writing-workflow evidence; the earlier automated form
observation was inconclusive and is not a continuing blocker by itself.

## September 19 history queries and offscreen qualification

New local macOS brokered artifacts use history v2: validated text and time filters
reach the existing store worker before candidate/result limits. Searches use the
store's word-prefix FTS semantics, remain read-only and profile-scoped, and return
at most 100 results. No additional startup worker or background document is added.
The new transform identity binds the adapter bytes; recorded v1 artifacts retain
their original recipe and reopen without migration or permission expansion.

Regression checks cover older matches behind 5,000 unrelated visits, Unicode,
time boundaries, profile isolation, granted-history authority, wire bounds and
legacy reconstruction. Distribution (95), service (214), app extension (56),
frontend (79), focused core/store/engine and JS contract tests passed, as did
desktop/probe Clippy and signed release packaging. Fresh Vimium 2.4.2 installation
in the isolated release QA profile returned a real history entry in its overlay;
this confirms live wiring, not unlimited Vimium history or Windows qualification.

On macOS 26.6.2, the native offscreen probe could not retain the required native
permission. A sandboxed extension iframe exposed extension APIs and successfully
wrote extension storage despite blocked DOM localStorage. Both probes released
their native objects and granted no product authority. Offscreen remains
unsupported: a fallback needs a dedicated isolated document host and message
bridge. Near-term work prioritizes smaller reusable API gaps instead.

## September 20 native filters and history delivery correction

`macos-web-extension-probe --content-script-globs-gate` tested a baseline script,
an include-filtered script and an exclude-filtered script on two loopback pages.
All three ran on both pages: this runtime ignores `include_globs` and
`exclude_globs`. Native objects were released. Local admission continues to reject
both declarations; a regression covers that refusal. No page-time workaround or
production probe was added.

Correction to the previous history evidence: the v2 recipe hashed the new adapter
but still emitted the v1 script. The earlier Vimium overlay check proved history
retrieval, not query-aware delivery. New v3 artifacts now replace the generated
bridge with the query-aware bytes. The v1/v2 recipes stay immutable for offline
reopening; existing installations keep those bytes until a new artifact is
installed. Tests inspect the prepared and reopened script, and cover both legacy
recipes and the v3 permission boundary. The original Vimium CRX prepared/reopened
successfully; executing its actual output adapter in a JS harness produced the
expected filtered-query wire request. This is not a new live browser workflow
qualification. Distribution (96), service (214), focused authority and JS checks,
plus desktop/probe Clippy passed. Selected-session restoration remains next work.

## September 25 integration and compatibility

Main was merged locally as `f720da15`. Both PROFILE v14 histories now converge
to v22 without rewriting main's migration SQL; the prior extension changes remain
uncommitted. The integrated frontend passes its checks/build and 150 WebKit
component tests. The external QA configuration explicitly uses `browser.html`.

Resource paths now preserve a conservative Unicode subset with bounded collision
checks across acquisition, sealed storage and runtime plans. The native WebKit
probe executed a Cyrillic-named script. Grammarly 14.1332.0 now passes archive/tree
validation and reaches a precise Unsupported result: identity, side panel and
three glob-filtered content-script groups. Its 191 script patterns fit existing
matcher budgets; the parser no longer confuses those with the 64-host grant limit.
Short names use the bounded 75-character name ceiling, not Chrome's recommended
12-character display length. Permission ceilings and glob refusal remain intact.

A separate capability profile delivers declared search/history/sessions adapters
without unrelated history/storage/background requirements. Prior history recipes
remain immutable. Sessions v2 lists and restores identified, timestamped closed
tabs in the focused regular profile/space; it does not restore windows, synced
sessions or full tab navigation stacks, or invent native numeric tab IDs. Old
records without metadata retain the ordinary browser reopen path.

Original Tab Restore 0.1.3 installed in the isolated signed QA build, activated
after the new native grant-schema mapping was corrected, and listed records across
a browser restart. A native restore reopened a tab. Explicit older-row selection
passes backend and original-popup JS tests; the user also confirmed the native
mouse action reopened the older Example Domain entry. Google
Translate 2.0.17 still requires unsupported offscreen audio. No polling, synthetic
background worker or hidden document was added for these capabilities.
