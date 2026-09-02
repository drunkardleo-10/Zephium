# M2 context identity and lifecycle core

Status: pure domain contract, bounded registry, closed shell/native port, and
the initial feature-gated macOS and Windows owned-context lifecycle adapters
are implemented. Both retain exact shell-requested navigation, one-shot
renderer-loss detection, same-view recovery, explicit close, and resource
accounting. The initial Windows selected-profile-to-automation-subprofile
cookie transaction is now host-reachable under that feature, but all Windows
behavior remains cross-compiled only. Same-document page-driven location
replacement and the bounded identity-bearing redirect contract are
host-reachable. Native redirect qualification, cross-document page-driven
navigation, presentation, borrowed/handoff native adapters, and named-device
qualification remain pending.

This evidence records code properties only. It does not claim that either
owned native adapter has passed a live production-context qualification, or
that the Windows cookie bridge, borrowed-tab native lease, or native sign-in
handoff is enabled in the shipping desktop graph.

## Implemented boundary

- `ContextId` and `ContextRunId` are canonical durable ULIDs. Debug output is
  redacted; native objects, `ItemId`, URLs, paths, page data, and provider data
  are absent from the contract.
- `ContextIdentity` permanently joins context, owning run, selected
  `ProfileId`, and one closed kind: owned, borrowed tab, or human sign-in
  handoff.
- Every asynchronous operation carries context generation, navigation epoch,
  main-frame identity and generation, run-cancellation generation, operation
  id, and operation class. Late, substituted, or duplicate settlements fail
  exact comparison.
- Ownership, presentation, exclusive input control, suspension, observation
  freshness, native residency, and terminal disposition are independent
  fields. Showing or hiding a page cannot adopt it or transfer input.
- Navigation, suspension/resume, renderer loss/recovery, human takeover,
  cancellation, close, release, and adoption invalidate the required
  generations without wrapping. Exhaustion seals automation but retains a
  teardown path.
- Human takeover preempts a pending navigation, keeps the same durable context
  identity, and requires a complete fresh observation before automation can
  resume. Renderer recovery restores pre-crash human control instead of
  silently returning input to an agent.
- Run cancellation is sticky, makes old results stale, and never takes input
  from a person. A cancelled owned context can only proceed to close; a
  borrowed or handoff context can only proceed to release.
- Owned adoption requires explicit human control. Borrowed and handoff
  contexts cannot adopt; borrowed contexts cannot be destroyed through the
  owned-context close transition.

The aggregate is a functional core. Construction creates no timer, thread,
native page, queue, or background work, so the unused product has zero runtime
agent overhead.

`ContextRegistry` is the single-owner admission and accounting boundary. It
retains at most eight live rows, at most four execution permits, and never
evicts. Rows above the execution ceiling remain queued; successful suspension
and renderer loss release a permit, while resume and deferred recovery must
reacquire one. A separate owned-native reservation count distinguishes new
owned/handoff resources from borrowed-tab leases. Terminal rows remain until
the shell observes an exact `Destroyed`, `TransferredToBrowse`, or
`ExistingBrowseRetained` disposition.

The shutdown seal permanently rejects admission, returns the bounded exact
never-started cleanup cohort without dropping those rows, and reports
quiescence only after queued authority is explicitly cancelled and every
active row reaches and exposes its terminal disposition. Bounded run/profile
indexes support cancellation and profile-erasure barriers without granting
authority.

`AgentBrowserPort` is a closed imperative boundary. Its request and event
vocabulary carries exact joins, validated web navigation targets, typed native
failures, paired platform construction attestations, post-revocation
cancellation, and a validated native resource audit. It cannot represent page
JavaScript, selectors, DOM data, CDP, native handles, cookies, profile paths,
headers, provider data, or platform error strings.

Construction requests must match immutable context kind, complete capability
kind, and an owned/borrowed/handoff source. Borrowed tabs use a redacted,
process-local lease identity rather than exposing `ItemId`. Successful native
construction cannot settle without one exact paired storage/extension proof:
macOS owned selected-profile storage with no extension controller or script
principal; Windows selected-profile or stable automation-subprofile storage
with an empty extension inventory; or an explicit borrowed/handoff proof that
normal Browse extensions remain active.

Native requests are bounded to at most sixteen retained tasks across the eight
live contexts. Resource audits contain only validated counts and reject limits
or contradictions. Navigation targets reuse the browser's 8 KiB absolute URL
gate and reject credentials, local files, internal principals, and dangerous
schemes; their debug representation is redacted. This gate is input validity,
not run policy authorization.

Every native construction request now carries an exact `ContextProfileLease`;
an otherwise valid context join cannot be dispatched with a lease for another
context, owner, profile, or kind. The lease also carries the authoritative
durable-or-ephemeral storage class, so a native adapter cannot infer a default
partition from `ProfileId` or substitute persistent storage for a private
profile. The single-owner lease registry retains at most eight active leases,
never substitutes a default profile, rejects lease and context identity reuse,
and exposes only redacted identities and bounded counts. Profile deletion first
installs a permanent profile tombstone, then returns the stable exact cleanup
cohort; active leases remain until their native resources settle. The shutdown
seal follows the same retain-until-exact-release rule and reports quiescence
only at zero leases.

Visibility and input ownership now settle against exact native callbacks.
Refused show/hide operations leave presentation unchanged. Human takeover
immediately revokes agent input and stales pending presentation/navigation;
native refusal never silently returns input to the agent. Returning input also
requires exact native success and then a complete fresh observation.

The Windows cookie bridge now has a closed native-only contract and bounded
correlation registry. A request contains one to eight canonical HTTP(S)
origins, exact current context joins and profile leases, and no cookie values.
It is valid only for a clean owned context attested as a Windows automation
subprofile. Post-auth handoff additionally requires an exact same-run,
same-profile Windows human-handoff context with export capability. At most two
transfers may be pending, and only when their destination profiles differ;
distinct destination contexts on one WebView2 profile are the same physical
cookie store and therefore cannot mutate concurrently. Each transfer accepts
at most 256 unique native-exposed cookies, 16 KiB per cookie, and 512 KiB total
field data. It also retains one exact process-local request/deadline interval:
the interval must be nonempty and at most 30 seconds, including the Windows
adapter's ten-second cleanup reserve. The functional core receives ticks from
the trusted shell but owns no clock, timer, or worker. Exact settlement
correlation includes this window, so a callback cannot substitute or extend a
previously admitted deadline.

The adapter contract requires an enumerate/validate/deduplicate preflight with
zero destination writes before application. Terminal outcomes distinguish
complete success, refusal with zero writes, and partial application. Partial
application is never treated as usable authentication state: the owned
destination must be destroyed/recreated before navigation. Results carry only
origin/cookie/HttpOnly/byte counts and closed failures. Reverse sync and
local/session-storage copying are not representable.

The engine now contains the platform-neutral production preflight owner, a
feature-gated WebView2 adapter, and the private host transaction that joins it
to exact source and destination profile authorities. Native UTF-16 fields are
decoded strictly into zeroizing bounded storage before cookie-shape validation;
malformed UTF-16 is rejected rather than replaced. The preflight has no
cookie-field diagnostic or serialization surface, bounds both raw observations
per origin and the unique cohort, rejects contradictory duplicate identity
snapshots, and permanently poisons a cohort after any admission failure.
Native cookie handles become available only through a sequential apply owner
after all requested origins complete.

The Windows adapter is callback-driven and has no event-pumping wait, worker,
channel, page script, or generic Wry cookie conversion. It queries each exact
origin sequentially, refuses an over-limit native list before indexing it,
copies native cookie objects through the destination manager without writing,
and starts `AddOrUpdateCookie` only after the whole cohort passes preflight.
An in-flight write bit closes the re-entrant cancellation window: a successful
native write is accounted before cancellation can settle. Failure after a
confirmed write first deletes cookies, then clears the destination profile's
browsing data, then requires a profile-wide null-URI `GetCookies` readback with
count zero. The private terminal distinguishes proven from unproven cleanup so
the host can quarantine the stable automation profile when proof is absent.
Dropping the owner requests shutdown cancellation rather than abandoning
callback-held state.

This seam deliberately does not use Wry's generic Windows cookie helper. The
pinned helper allocates from the native-reported count, loops that complete
count, silently drops conversion failures, blocks through an event-pumping
wait, and returns cookie values to its caller
([pinned source](../../vendor/wry/src/webview2/mod.rs)). The production adapter
uses profile-scoped WebView2 cookie managers with bounded native callback
ownership and an application deadline that reserves ten seconds for cleanup.
It cannot accept a free scope or native deadline: both are derived from the
exact correlated request, with its functional duration anchored to the
port-admission `Instant`. Queue time therefore consumes the same terminal
window, and clock regression or an already elapsed interval refuses before
enumeration. The move-only native task captures that anchor only after its
bounded port permit is acquired and retains it beside the exact request across
main-thread queueing. The public engine port accepts cookie transfers only on
Windows when `agentic-browser` is compiled; every other target remains
mechanically unsupported, and the ordinary shipping graph still does not
enable that feature.
Microsoft documents that cookie-manager changes
apply to the user-profile context, `GetCookies` is URI-scoped, and
`AddOrUpdateCookie` applies a native cookie; the native cookie object exposes
`IsHttpOnly` for exact preservation
([CookieManager](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2cookiemanager),
[Cookie](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2cookie)).
`ClearBrowsingDataAll` clears the entirety of the selected profile and invokes
its completion only after that asynchronous operation settles; closing the
associated WebView early may release the handler without invoking it, which is
why host retention through terminal cleanup is mandatory
([Profile2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile2)).

The same profile-scoped behavior is enforced as a host-owned transaction.
Destroying one owned WebView does not remove cookies already written to the
stable `agent-<ProfileId>` profile, and multiple WebViews on that profile share
those values. The host therefore admits at most two native transfers globally
and only one per logical destination profile. It derives the read-only source
cookie manager from an existing ordinary Browse view or spare whose exact
logical profile and retained WebView2 environment match; no `ItemId`, cookie
field, or extension principal crosses that isolated source seam. It derives
the destination only from the exact clean, hidden, active automation context
after re-attesting its join, profile lease, import capability, extension
inventory, storage, owner, suspension bit, semantic idleness, and environment.
Navigation, recovery, suspension, resume, policy replacement, renderer loss,
cancellation, erasure, close, audit, and shutdown all account for the retained
transaction.

Every partial outcome permanently contaminates that exact context, including
one whose profile-wide empty readback succeeded, so only close and recreation
can return it to navigation. When cleanup is unproven, a process-sticky
profile quarantine additionally refuses later automation-subprofile
construction for that logical profile while leaving ordinary Browse and
extension principals untouched. The native task, exact admitted request,
watchdog, destination binding, cleanup receipt, and terminal settlement remain
in one bounded host bijection until terminal cleanup or fail-stop teardown.
`ICoreWebView2Profile8::Delete` is not an immediate recreation primitive:
Microsoft documents that the name remains delete-pending until the browser
process exits. The adapter currently cross-compiles but is not exported to the
shipping desktop graph, and no physical Windows execution has occurred. This
evidence therefore makes no runtime cookie-write, cleanup-qualification, or
shipped-support claim. The post-auth handoff direction also remains a typed
`SourceUnavailable` refusal until a native temporary-handoff source exists.

For extension inventory, an empty `GetBrowserExtensions` result is accepted
only when extension support was enabled for the inventory environment. The
API explicitly returns no extensions when `AreBrowserExtensionsEnabled` is
false, so runtime disabling cannot prove an empty profile
([GetBrowserExtensionsAsync](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2profile.getbrowserextensionsasync)).

The sign-in handoff skeleton is an exact-join functional workflow, not a UI or
implicit retry loop. It admits only a distinct same-run/same-profile temporary
normal context, validates paired platform construction proofs, transfers input
exclusively to a person, requires explicit human completion and exact return
of input, then selects the truthful storage path: shared WKWebsiteDataStore on
macOS or the bounded one-way cookie bridge on Windows. The temporary context
must release before a scoped owned-context refresh, and completion requires a
fresh automatable registry entry for the exact owned join.

Every terminal callback is consumed into a terminal workflow phase. A wrong
construction proof or committed refresh outside the approved origin cohort
blocks rather than leaving an in-flight state. Cookie refusal is a typed
blocker; partial application marks the destination contaminated and cleanup
requires it to close. Cancellation clears workflow-level pending authority but
reports completion only from exact `RetiredContext` proofs: owned destroyed as
closed and handoff destroyed as released.

## Initial macOS native adapter

`zephium-engine` now exposes the production adapter only through the separate
`agentic-browser` feature. That feature depends on the shipping functional core
but cannot activate `probe-harness` or `native-agentic-input-probe`; the
ordinary desktop graph does not enable it yet. If no caller takes the unique
port, no queue state, worker, timer, page, or native context is created.

Taking the port creates one bounded admission state and is exact-once. At most
16 requests can be retained across the outer main-loop dispatcher and the
engine's dedicated, non-coalescing 16-task host band. Rejection releases its
permit without manufacturing an asynchronous result; every accepted task
either rejoins the main-thread host and emits one request-correlated terminal
event or emits one closed native-refusal result when its retained owner is
dropped. Poisoned admission is sticky, closes the port, and invokes the
mandatory fatal callback once rather than deadlocking.

The first macOS adapter accepts owned construction, one exact shell-requested
navigation at a time, exact renderer recovery, exact close, post-revocation
cancellation, and privacy-preserving resource audit. Cookie transfer remains
synchronously unsupported. Owned construction:

- binds the exact context identity, complete capability inventory, profile
  lease, and authoritative durable/ephemeral storage class;
- reuses the engine's process-lifetime private `WKWebsiteDataStore` per
  ephemeral profile, proves distinct private profiles do not alias, and uses
  the exact profile data-store identifier for durable storage;
- creates only a fixed 1280-by-800 logical, non-autoresizing, hidden, unfocused
  `about:blank` child with permissions, downloads, popups/page-close, media
  surfaces, autofill, link previews, and inspection denied;
- asserts the actual `WKWebViewConfiguration` has no extension controller and
  exactly one pointer-identified document-start script: the pinned private
  semantic runtime in its dedicated `WKContentWorld`; no ordinary Zephium or
  extension script principal is present. It then installs the current native
  content blocker before any network navigation can be represented;
- retains the content-policy registration, page, and native resource lease in
  teardown order under a private `ContextId` map that is absent from ordinary
  tab, session, stage, navigation-snapshot, and extension-principal maps.

Navigation advances only the exact functional-core navigation/frame successor
and retains its accepted port task until a native commit or closed refusal.
macOS and Windows use one shared navigation state machine rather than platform
copies. Exact-target/no-redirect remains the default. A request may instead
carry a trusted-shell redirect scope containing at most eight canonical
origins; the adapter then accepts at most eight redirect observations under
the exact native navigation identity. The construction-only `about:blank`
permit is one-shot, and unarmed page navigation remains denied. Commit and a
30-second watchdog share one atomic terminal claim, so exactly one can enqueue
the settlement. A committed native `NavigationId` and authoritative final
target retain one content-free `Finished` fact even after logical settlement;
duplicate, unobserved-target, over-limit, or post-failure completion is an
invariant failure or closed refusal. This provides the same exact readiness
evidence to both platform adapters without retaining a page value, timer, or
additional queue entry. One navigation-or-recovery page-load terminal and one
renderer loss can each contribute at most one callback per live context, so
native callbacks own an independent fixed 16-entry host band; they cannot
compete with the 16-entry request band.
Cancellation, close, and shutdown stop loading, retire the exact native gate
and watchdog, and terminally settle the retained navigation before releasing
its queue permit. Resource audits subtract in-flight operations from
request-queue depth and add the independently bounded terminal depth rather
than double-counting either.

The pinned Wry WebKit process-termination callback is installed on the private
view and joined to its construction-time origin join. Its platform
gate claims renderer loss once, revokes any armed navigation terminal, and
denies all later navigation. The host retires and settles an accepted
navigation before emitting the closed `RendererLost` event for its exact prior
join. Duplicate or stale old-view callbacks are no-ops; a lost renderer reduces
`resident_views` to zero while its exact binding, view object, policy
registration, profile lease, and resource reservation remain owned for close
or recovery. Close and cancellation accept only the exact one- or
two-generation functional-core rejoin required by the race between a native
loss callback and its shell observation.

Recovery reuses that already-attested WKWebView, configuration, selected
profile store, policy registration, and native reservation rather than
temporarily constructing a second page or storage binding. The host retains
the last exact committed web target. Before rearming, it reasserts the actual
configuration's absent extension controller and scripts, selected durable or
pointer-identical ephemeral store, fixed logical frame/autoresizing mask,
disabled inspection, and hidden state. A
lost initial `about:blank` reloads only that internal document, while a lost
web document uses WKWebView's native reload and the same exact URL policy. The
recovery operation is admitted only at the double full-generation successor
produced by renderer loss followed by `begin_recovery`. Its load commit and
30-second watchdog share the same atomic terminal claim as ordinary
navigation. `resident_views` remains zero until an expected target commits
under the exact Wry navigation identity, after which the functional core still
requires a complete fresh observation. A timeout, load failure, wrong target,
cancellation, or second pre-commit termination refuses recovery and leaves
automation fail-closed; a termination that follows an already-claimed recovery
commit remains queued as a newer loss and cannot be erased by terminal
settlement.

The core already caps each canonical target at 8 KiB. Recovery retains one
steady target per live binding and at most one temporary clone per in-flight
recovery, so the eight-context ceiling bounds this adapter's serialized target
retention to 64 KiB steady and 128 KiB during the all-context recovery peak.
It creates no replacement view, profile store, policy registration, thread,
or additional native-resource reservation.

With `agentic-browser` enabled, the native resource ledger owns an independent
eight-context class equal to the functional core's live-context ceiling. Its
accounting reserve changes that feature graph's declared hard ceiling from 48
to 56 without allocating a native object or changing any existing class
ceiling; the ordinary feature-disabled ledger remains the original seven-class,
48-slot layout. Audits cross-check the private map against the physical agent
class and expose only bounded counts. Content-policy replacement includes every
same-profile owned context in the preconstructed native replacement cohort.

Profile erasure rejects an active private context after installing its sticky
tombstone, preserving the exact storage obligation for orderly close and
retry. Shutdown seals the port before host teardown, physically destroys any
remaining context, and refuses a clean result if the shell failed to settle
every exact Close first. This is static/unit evidence only; no new GUI run,
external site, account, credential, global input, or Accessibility authority
was used.

## Initial Windows native adapter

The same dormant production `agentic-browser` port now admits owned Windows
contexts. This is source, unit, and MSVC cross-compile evidence only: no
WebView2 controller was constructed and no behavioral claim is inferred from
the target build.

Construction first binds the exact logical profile to its durable profile root
or process-lifetime private runtime root. It then establishes or rejoins the
engine's extension-enabled WebView2 environment at that exact UDF. An existing
extension-disabled environment is a typed restart/profile-busy refusal; it is
never reinterpreted as empty-inventory proof. This preserves WebView2's
immutable environment option contract: Microsoft documents that changing
`AreBrowserExtensionsEnabled` while a matching environment is already running
fails with `ERROR_INVALID_STATE`
([EnvironmentOptions6](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environmentoptions6)).

For a durable selected profile whose bounded native inventory is empty, the
owned controller reuses that exact profile. If the selected inventory is not
empty, or the context is ephemeral, construction selects the deterministic
`agent-<ProfileId>` automation subprofile within the same environment/UDF and
requires that exact controller profile to have an empty inventory. Multiple
WebView2 profiles in one UDF isolate cookies and storage while sharing the
environment/process group; this avoids one extra UDF and browser process per
agent context
([WebView2 multi-profile support](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/multi-profile-support)).
An empty inventory is accepted only through `ICoreWebView2Profile7` on the
extension-enabled environment; Microsoft explicitly documents that extension
enumeration otherwise returns no extensions when extension support is disabled
([Profile7](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile7)).

The pinned Wry startup gate runs after creating the exact controller/profile
but before WebView initialization, script installation, or initial navigation.
It reattests the environment UDF, controller/environment identity, exact
profile name, InPrivate bit, and bounded empty extension inventory. The adapter
supplies no initialization script, IPC handler, host object, custom protocol,
new-window callback, selector, CDP method, or native-input route. It constructs
one fixed 1280-by-800 logical hidden and unfocused `about:blank` child with
devtools, clipboard,
permissions, downloads, popups, page-close, autofill, context menus,
accelerators, autoplay, fullscreen, and picture-in-picture denied. Before
publication it rechecks the parent/container/controller HWND lineage, hidden
controller and window state, DPI-derived container and controller bounds,
absence of focus, exact browser-process
provenance, and installation of the ordinary native content policy.

The controller then enters only the private `ContextId` map; ordinary tab,
session, stage, suspension, navigation-snapshot, and extension-principal maps
remain untouched. Navigation uses the shared exact one-at-a-time state machine:
one construction bootstrap, canonical initial-target match, native Wry
`NavigationId`, bounded explicit redirect scope and hop count,
commit-or-timeout atomic claim, authoritative final-target settlement, one
post-settlement `Finished` fact, renderer-loss seal, and recovery-only rearm.
Windows deadlines use at most 16
fixed UI-thread `SetTimer` slots, allocate no worker, channel, or map, and have
zero timer activity while idle. Native callbacks use the independently bounded
terminal host band and cannot replace an accepted request debt.

Windows owned contexts now implement the functional core's hidden suspend and
resume transitions through `ICoreWebView2_3`. Admission requires the exact full
generation successor, the owned `Suspend` capability, no navigation, recovery,
semantic, or prior suspend work, and the already-attested hidden/unfocused
owner. `TrySuspend` is best effort and has no cancellation primitive, so a
fixed ten-second UI-thread watchdog and the native callback share one atomic
claim. The winner owns the sole external transition settlement. A callback
that loses to timeout or logical cancellation remains a private cleanup debt:
the host blocks navigation, recovery, policy replacement, further suspension,
and clean resource audit until it calls `Resume` and proves the final native
bit active, or destroys/loses the exact view. Renderer loss, close, and shutdown
retire that late callback authority before releasing the binding.

The adapter rechecks hidden HWND/controller ownership before recording the
final `IsSuspended` bit. Resume is not inferred from its HRESULT: the host calls
`Resume`, reattests hidden ownership, then reads the bit again. This ordering is
intentional because Microsoft documents that `TrySuspend` is best effort,
`IsSuspended` is true only from successful completion until resume, and some
WebView APIs can implicitly resume a suspended view
([ICoreWebView2_3](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_3)).
Resource snapshots count only settled suspended residents; they refuse while a
suspend or unreconciled late callback can make the physical count ambiguous.
Content-policy replacement refuses before touching any suspended or uncertain
Windows owned view, so a blocker update cannot silently auto-resume it.

This is compile-time, pure-state, and MSVC cross-target evidence only. No
physical Windows suspend/resume behavior, process-memory reduction, timer
latency, renderer-loss race, or long-running-script case has been qualified.
macOS remains explicitly unsupported at the native port because no equivalent
public `WKWebView` suspension/readback primitive has been proven for the pinned
engine. The release-excluded Windows semantic/lifecycle qualifier includes a
create-new hidden mode that runs the same production `TrySuspend` adapter,
final suspended and resumed readbacks, and a fresh same-document semantic
observation. Its closed offline reviewer cannot promote support until that
record and the other five exact records pass on one matching physical Windows
runtime.

Close, cancellation, profile erasure, and shutdown stop loading and settle any
retained navigation before retiring content policy and calling WebView2 close.
A failed close transfers the exact native-resource lease to the existing
bounded, profile-attributed cleanup-debt owner; capacity is not reissued while
the controller/HWND teardown is unproven. An accounted debt makes the close
settlement unclean and quarantines that profile, but is not mislabeled as an
accounting invariant failure. Unaccounted or overflowed debt remains a sticky
host failure. Browser-process loss preserves the exact fail-closed context
identity; the current same-controller recovery may refuse after a complete
browser-process death, after which the shell must close and reconstruct rather
than silently substituting a new native owner.

## Deterministic evidence

```sh
cargo test --locked -p zephium-agentic
cargo clippy --locked -p zephium-agentic --all-targets -- -D warnings
cargo test --locked -p zephium-engine --features agentic-browser --lib
cargo clippy --locked -p zephium-engine --features agentic-browser \
  --all-targets -- -D warnings
cargo check --locked --target x86_64-pc-windows-msvc \
  -p zephium-engine --features agentic-browser
cargo check --locked --target x86_64-pc-windows-msvc \
  -p zephium-engine --features agentic-browser --tests
cargo check --locked --release --target x86_64-pc-windows-msvc \
  -p zephium-engine --features agentic-browser
cargo xtask check-agentic-probe-boundary
```

The tests cover canonical/redacted identity, kind-scoped capability sets,
construction and navigation correlation, stale settlements, exact
presentation settlement, visibility and ownership separation, takeover,
cancellation, suspension, deferred renderer recovery, adoption/release
exclusivity, non-wrapping exhaustion, both registry ceilings, no-eviction
pressure, terminal resource disposition, shutdown quiescence, closed native
request classes, construction-proof compatibility, unsafe URL rejection,
redacted debug output, and bounded/consistent native resource snapshots.
Redirect tests additionally cover canonical/deduplicated/redacted origin
scope, exact no-redirect defaults, wrong native identity, absent policy,
same-origin and cross-origin permitted targets, opaque macOS redirect URLs,
unobserved final-target substitution, the eight-hop ceiling, authoritative
final settlement, and sign-in refresh preflight against the already-approved
cookie-origin cohort.
Profile-lease tests cover exact selection, kind compatibility, duplicate and
capacity refusal without eviction, durable/ephemeral storage retention, stale-
release rejection, deletion races, and shutdown quiescence.
Cookie-transfer tests cover canonical/deduplicated origins, Windows clean-
subprofile proof, handoff owner/profile joins, required capabilities, payload
and HttpOnly count invariants, nonempty/reversed/over-ceiling native windows,
deadline redaction and exact settlement, explicit partial application,
destination-profile single-flight (including distinct-context alias refusal),
process concurrency across distinct profiles, redacted debug output, and
shutdown drain. Engine preflight tests additionally cover exact duplicate and
conflict handling, all-origin release, sequential apply accounting, invalid
native shapes, zero/overflow origin and cookie ceilings, raw duplicate-flood
refusal, strict UTF-16 refusal, write-ticket accounting, and content-free
diagnostics.
The release boundary mutation-tests the Windows adapter's non-shipping feature
gate, exact request-derived scope/deadline mapping, admission-inclusive queue
time, count ceiling, destination-side copy, write interlock, sequential
accounting, profile-wide cleanup/readback, secret conversion, and absence of
thread, message-pump, script, CDP, serialization, or direct diagnostic
surfaces. It separately pins the host's exact profile/environment source join,
automation-only destination, global and profile-local concurrency ceilings,
deadline watchdog, context contamination, unproven-cleanup quarantine,
cancellation, shutdown ownership, and absence of direct cookie reads in the
ordinary Browse projection. It also pins suspension's target/feature gate, atomic
callback/timeout/cancellation ownership, late reconciliation, fixed deadline,
native readback, resource count, teardown retirement, and refusal to replace a
content policy on a suspended view. The adapter additionally cross-compiles
against the pinned Windows target; runtime behavior remains a physical-Windows
evidence item.
Handoff tests execute complete macOS shared-store and Windows cookie-bridge
flows through the real context registry, including exclusive human control,
temporary-context release, scoped refresh, fresh observation, incompatible
proof, out-of-scope commit, partial-transfer contamination, stale operation,
redaction, and exact cancellation resource dispositions.

The engine suite also exercises the platform-neutral exact navigation gate and
statically proves Windows construction orders environment/profile proof,
transient-resource acquisition, native construction cleanup import, process
readback, content policy, resource reclassification, and private-map
publication. Rejected unpublished views must close and transfer any failed
teardown with their exact lease before quarantine or return. The release
boundary independently rejects Windows production source that acquires a page
script/IPC/CDP/selector/native-input surface, an unbounded timer authority, or
loses its hidden/profile/inventory/process/policy/cleanup checks.

## Bounded native location replacement

Owned contexts now observe same-document native location/history replacement
without adding page-world JavaScript, selectors, DOM access, CDP commands, a
worker, a channel, or an unbounded event queue. The macOS adapter retains the
existing KVO-backed `WKWebView` URL/history observer beside the private view;
the Windows adapter retains `SourceChanged` plus `HistoryChanged` registrations
on the exact WebView2 controller. Both registrations are removed before their
native view can be released.

The shared navigation controller opens observation only after the exact web
document reaches its native `Finished` event. Four booleans represent all
runtime state: readiness, one claimed callback, one coalesced dirty fact, and
one emitted replacement awaiting shell rejoin. Repeated or continuously
generated History API signals therefore retain no URL cohort and can own at
most one terminal callback per context. A signal between commit and finish is
collapsed into the same dirty fact. Page-load arms, recovery, renderer loss,
cancellation, and teardown close observation rather than allowing it to widen
navigation authority.

The host samples only the native current URL through the existing 8 KiB
UTF-16/UTF-8 bound, reparses it as `ContextNavigationTarget`, and compares the
canonical `SemanticOrigin` with the last exact committed target. An unchanged
URL consumes the signal. A same-origin path/query/fragment replacement updates
the private committed target and emits the already-closed
`ContextNativeEvent::NavigationReplaced` against the exact prior join. A
cross-origin, malformed, missing, or bootstrap substitution stops loading and
fail-stops the adapter; it is never interpreted as a redirect grant. Unarmed
network navigation and redirects without an explicit operation-local scope
remain denied by the exact Wry navigation gate.

The host holds the replacement until a later request proves the functional
core's deterministic successor. Direct observation/screenshot/cookie work must
carry the one navigation/frame successor; a new navigation carries the double
navigation/frame successor; cancellation, suspension, or close carries the
replacement followed by one full generation advance. A later native location
signal remains one dirty bit and forces that request to settle stale before a
second replacement can be emitted. Renderer loss racing an unjoined
replacement is likewise held until the replacement successor is proven, so a
loss event is never mislabeled with the pre-replacement join; exact close may
consume both barriers without manufacturing a redundant loss event.

Semantic and screenshot completions recheck that no native location signal,
page load, replacement rejoin, or renderer loss raced their result. The host
also clears the current semantic snapshot authority and terminally stales an
owned screenshot as soon as it records a replacement. Native lifecycle work
temporarily defers the single location sample until its exact terminal; it does
not rewrite or overtake the lifecycle settlement.

This is unit, static-boundary, and macOS/Windows compile evidence. The
release-excluded Windows semantic qualifier now includes a dedicated physical
same-document mode, but that mode has not run on a Windows device and does not
promote presentation support. Its loopback document reaches native load
completion before requesting a single host-held script. The host releases only
one fixed `history.replaceState`; the qualifier then requires the exact bounded
native `Source`, same-origin target, functional-core navigation/frame successor,
stale-prior refusal, native replacement rejoin, drained callback/dirty state,
and a fresh post-replacement semantic snapshot. The delayed script shape avoids
making WebView2's `NavigationCompleted` wait on the held response. Microsoft
documents that `NavigationCompleted` coincides with `body.onload`, while
`SourceChanged` covers same-page URL changes and `HistoryChanged` covers joint
session-history changes
([navigation events](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/navigation-events),
[CoreWebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2)).

The same qualifier also retains and settles ordinary full-navigation
`SourceChanged`/`HistoryChanged` claims against the exact committed native
`Source`; it refuses a pending, dirty, substituted, or teardown-surviving claim.
Together these checks prepare the exact physical evidence path without claiming
the still-unrun Windows behavior.

## Bounded identity-bearing redirects

`ContextNavigationRequest` remains exact and redirect-free unless the trusted
shell attaches an immutable operation-local `ContextNavigationRedirectPolicy`.
The policy stores one to eight sorted, unique `SemanticOrigin` values and has a
redacted debug representation. It is a projection of already-approved run
policy, not authority supplied by a model or page. The human sign-in refresh
workflow preflights every redirect origin against its exact cookie-origin
scope before it enters a pending state.

The native controller first binds `Started` to the exact requested target and
then accepts at most eight `Redirected` events carrying that same native
navigation identity. Exact requests refuse even a redirect back to the same
URL. A scoped request can settle only after at least one observed redirect and
an authoritative `Committed` URL that is either the exact requested target or
inside the retained origin scope. A changed commit without a preceding
identity-bearing redirect is refused. The host retains and returns that actual
final target, not the initially requested URL, and stops loading on every
refusal.

On WebView2, Wry obtains URI, `NavigationId`, and `IsRedirected` from one
`NavigationStarting` callback and preserves the last admitted URI through
commit and completion. On WebKit, intermediate redirect callbacks preserve
the exact `WKNavigation` identity but intentionally report the registered
initial URL; the authoritative destination is sampled from `WKWebView.URL` at
commit. This is why the common controller treats redirect-event URL as a
policy check but native identity, hop count, and final commit as the
attribution proof. No public Wry API, model-facing JavaScript, selector, CDP
surface, generic native bridge, worker, channel, or idle task was added.

The synchronous engine URL policy refuses destinations outside the static
scope when the platform exposes them. This is an acceptance, presentation,
and settlement boundary, not a promise that a disallowed HTTP request emitted
zero network traffic: Microsoft explicitly notes that the GET may already be
in progress while `NavigationStarting` is handled. A rejected or over-limit
navigation is stopped and never becomes an accepted context result
([WebView2 navigation starting](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2navigationstartingeventargs),
[WebKit navigation action](https://developer.apple.com/documentation/webkit/wknavigationaction)).

The loopback fixture now contains a fixed relative two-hop redirect chain and
a fixed two-node loop, with no caller-selected `Location` value. Unit tests
and the release-boundary mutation gate cover the contract. The release-excluded
Windows semantic/lifecycle qualifier now has a sixth create-new hidden mode
that must prove the production gate's exact eight-hop refusal, subsequent
recovery, two-hop authoritative final commit, fresh semantic observation,
focus invariants, and full resource teardown. Neither native adapter has yet
run that fixture on a physical named-device qualification, so this evidence
makes no runtime redirect-support claim.

## Remaining M2 work

1. physically qualify Windows owned construction, selected/subprofile
   inventory, hidden/focus state, navigation, loss, close, debt, and shutdown
   on an explicitly authorized named Windows device/runtime;
2. physically qualify the bounded same-document replacement observer and the
   fixed redirect-chain/loop contract, then add presentation only with a
   dedicated non-overlapping Work surface owner;
3. physically qualify the bounded selected-profile Windows cookie transaction
   and its partial-cleanup/quarantine behavior, then add native
   borrowed/handoff source transactions without changing ordinary extension
   principals;
4. qualify macOS construction/storage/inventory/close and both-platform
   lifecycle, idle-resource, cancellation, recovery, and shutdown behavior on
   explicitly authorized named devices.
