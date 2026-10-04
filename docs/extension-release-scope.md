# Initial extension release scope

September 11, 2026. This is the working product scope from the current product
scope. It replaces the earlier marketplace, curated-package
publication and per-version approval plans, which are no longer in scope.
It describes the implementation target, not completed behavior. Existing
security guarantees and persisted ownership records must be preserved through
explicit changes and migrations.

## Product

Users visit the actual Chrome Web Store, request installation through an Add to
Zephium action, review permissions, and install directly into their local
browser profile. Packages and updates come directly from Google's service to
the device. Zephium does not mirror or redistribute packages through its backend.

A small manually maintained Tested with Zephium list is informational. It may
start as a static page with store links and specific tested workflows/platforms.
Absence from the list does not itself prohibit installation. Tested describes
completed testing, not a permanent compatibility or safety guarantee.

The first scope covers Chrome Web Store installation. Other stores, a built-in
discovery marketplace, Zephium-hosted builds, Mods, and per-version approval
infrastructure are later work. Broad compatibility is a continuing program;
this release's infrastructure must behave reliably for both supported and
unsupported extensions.

## Installation and local authority

The store-page adapter only requests installation of a canonical extension ID.
Rust owns request validation, bounded direct acquisition, original package and
publisher authentication, compatibility assessment, consent, durable install,
and native activation. Page text/JavaScript never establishes permissions,
package identity or trusted status. Requests must be bound to the current
profile and real user invocation, including navigation/cancellation races.

Keep the Add to Zephium behavior, but avoid making the installer depend on
Google's DOM structure. A browser-owned action for the current canonical listing
is a possible fallback if page integration breaks; both use one installer.
This is a UI implementation recommendation, not a second acquisition system.

Local package signatures, actual permission grants, supported platform behavior
and durable install state authorize ordinary execution. Membership in a tested
list or a fresh response from zephium.app does not. Introduce or refactor the
external-install authority explicitly; do not relax the existing exact Verified
witness or make an expired signed-policy witness silently valid.

Installed version, publisher identity, original digest and any applied local
adaptation remain local bookkeeping. Version recording and rollback protection
are not a product-maintained whitelist. Prefer adapting the runtime's APIs to
upstream behavior; keep package changes narrowly scoped and attributable where
they are genuinely required.

Unsupported security-sensitive requirements must refuse safely. An unsupported
optional feature should not automatically prohibit an otherwise usable package
when the platform can honestly deny that optional capability. Never silently
pretend an unsupported API or permission has been implemented.

## Lifecycle and updates

- Download/authentication, cancellation and consent must have coherent outcomes.
- Commit install and grants together, then activate through the existing native
  ownership protocol. Installation failure must not leave an apparent success.
- Restart restores enabled extensions locally. A metadata outage or expired
  recommendation feed must not disable ordinary installed extensions.
- Disable stops execution; uninstall stops execution and removes the extension's
  profile data. Updates/removal cannot race an unresolved native owner.
- Keep profiles isolated. Private browsing stays unavailable unless its isolation
  and permission behavior have actually been implemented and qualified.
- Updates use the original source, run authentication and compatibility checks,
  and request consent for new required access. Fetch failure leaves the current
  installation usable. Preserve data and recover atomically from interrupted
  updates; do not automatically run a known-revoked package as a fallback.
- Use bounded request frequency, retry backoff, jitter and conditional requests
  where supported. No package traffic through Zephium's backend, stable client
  identifier or server-side per-user extension inventory.
- Bound extraction, installed/orphaned storage and runtime resources. Reclaim
  temporary/obsolete objects only after durable ownership proves they are unused.

Optional signed security advisories can retain authenticity and rollback
protection. Keep recommendations, known revocations and installation permission
as separate concepts. Known authenticated revocations must not be erased simply
because a later refresh fails. This scope does not authorize enabling or changing
any deployed backend contract silently.

## Current code assessment

The `external-extensions` feature now connects the browser-owned store action,
canonical listing validation, direct Google CRX3 acquisition, explicit local
compatibility admission, permission review, durable installation and native
activation. The inspected listing supplies only the requested ID; original
package bytes supply authenticated publisher identity and manifest data.

Local admission and offline artifact reopening use a separate typed authority.
They do not require signed remote metadata or reuse the exact Verified witness.
Original package authentication, transformation provenance, permission grants and
upstream high-water history remain enforced. The backend public-policy parser
contract is unchanged.

The isolated `external-extensions-qa` debug bundle has now exercised real Dark
Reader 4.9.130 installation from Google, native popup/options presentation,
dynamic dark styling on example.com and IANA, live site toggling, native keyboard
commands, restart, disable/re-enable, and removal/reinstallation.  The normal desktop build still requires explicit
feature selection; this is not a completed release qualification.

The compatibility subset remains narrow. On-demand original-source update
checks, consent for additional required access, manager optional-permission
edits and bounded package reclamation are implemented. Real Dark Reader and
Vimium update checks returned Up to date, and Dark Reader's optional permission
grant, persistence and revocation were exercised. Signed synthetic packages
cover version-changing Store transactions; actual enabled native swaps and
interruption recovery still need qualification. Automatic source-update
scheduling, remaining extension-initiated permission paths, corruption-repair
presentation, native profile-isolation QA and sustained use remain outstanding.
The local WebKit adapted profile now selects existing implementations by
capability and carries separately authenticated publisher-native bindings.
1Password has user-observed workflow evidence after desktop trust setup, and
JSON Formatter has a live Raw/Parsed page workflow alongside the original two.
The shared runtime limit is twelve, with an independent three-profile limit;
this is an admission bound, not twelve-extension endurance qualification.
Vimium 2.4.2 now uses the existing WebKit compatibility compiler through the
local store path; link hints, navigation, history results and default search
have been tested alongside Dark Reader. Physical Windows validation has not
been performed. Do not delete installations
or unresolved native ownership records to simplify this work.

Stylus remains outside the current compatibility subset. Grammarly is deferred
to a later compatibility phase after an authenticated package inspection found
both a portable-path blocker and broader API/authority requirements. Neither
is on the tested list. See the runtime report for evidence and limitations.

## Implementation order

1. Align local external admission and offline restore with this scope, separating
   informational metadata from actual install authority. Preserve authentication,
   grants, rollback protection and existing Verified isolation.
2. Finish one vertical install flow: real store listing, direct original package,
   trusted consent, durable Store transaction, native activation and usable
   extension action. Connect to the existing serialized service and manager.
3. Complete restart, profile isolation, disable, uninstall and bounded cleanup
   through the same service. Resolve real platform capability gaps exposed by
   the initial extensions rather than adding mock API success paths.
4. Complete source updates, permission changes and interrupted-update recovery.
5. Simplify presentation to installed extensions, permissions/errors and a link
   to the small tested list. Remove initial-release dependence on curated
   discovery, release publishing and per-version verification UI.
6. Run the whole first-use and failure matrix in the actual packaged macOS app,
   using real representative extensions. Prepare Windows instructions only after
   the shared implementation and local qualification are complete.

## Completion evidence

The local/shared portion is complete only when all of these hold:

- A fresh profile can install from a real Chrome Web Store listing through the
  ordinary product UI, without developer provisioning or a fixture catalog.
- Tested extension workflows operate on real pages, including actions/options,
  navigation, reload and fresh tabs. A popup opening is insufficient evidence.
- Cancelled, duplicate, invalid and unsupported installs have clear, safe outcomes.
- Enabled state and extension data survive restart and offline startup.
- Disable, uninstall, update and permission changes behave correctly without
  leaking execution or data across profiles.
- Update/network failures and process interruption preserve coherent recoverable
  state. Integrity failures remain distinguished from compatibility problems.
- Resource bounds and sustained use are checked, and relevant automated gates
  pass. Existing engine security review expiry is refreshed from actual vendor
  evidence rather than bypassed.
- Windows code/tests are prepared and the exact native verification matrix is
  documented. Physical Windows results remain a separate claim for that agent.

The tested list records only completed platform/workflow evidence. A target of
wider API compatibility does not become a claimed percentage of working store
extensions.

## Google service terms

Being open source or early-stage does not establish an exemption from service
terms. A mandatory bespoke Google agreement has not been established either.
Keep the exact interface/updates/local-adaptation question documented for scoped
review; do not invent a Google partnership prerequisite or claim legal clearance.
The engineering plan remains the direct user-requested browser installation flow.
Alternative
hosting/build routes are outside this initial product scope.
