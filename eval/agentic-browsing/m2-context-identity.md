# M2 context identity and lifecycle core

Status: pure domain contract, bounded registry, closed shell/native port, and
the initial feature-gated macOS owned-context construction/close adapter are
implemented, including exact shell-requested navigation on that first adapter.
One-shot native renderer-loss detection, exact same-view recovery, and
resource accounting are also implemented. Redirect and page-driven
navigation, presentation, suspension, Windows native ownership/cookies,
borrowed/handoff adapters, and named-device qualification remain pending.

This evidence records code properties only. It does not claim that an owned
native context has passed a live host/device qualification, or that a Windows
cookie bridge, borrowed-tab native lease, or native sign-in handoff has shipped.

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

The shutdown seal permanently rejects admission, drops only never-started
rows, enumerates a bounded exact cleanup cohort, and reports quiescence only
after every active row reaches and exposes its terminal disposition. Bounded
run/profile indexes support cancellation and profile-erasure barriers without
granting authority.

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
transfers may be pending; each accepts at most 256 unique native-exposed
cookies, 16 KiB per cookie, and 512 KiB total field data.

The adapter contract requires an enumerate/validate/deduplicate preflight with
zero destination writes before application. Terminal outcomes distinguish
complete success, refusal with zero writes, and partial application. Partial
application is never treated as usable authentication state: the owned
destination must be destroyed/recreated before navigation. Results carry only
origin/cookie/HttpOnly/byte counts and closed failures. Reverse sync and
local/session-storage copying are not representable.

This seam deliberately does not use Wry's generic Windows cookie helper. The
pinned helper allocates from the native-reported count, loops that complete
count, silently drops conversion failures, blocks through an event-pumping
wait, and returns cookie values to its caller
([pinned source](../../vendor/wry/src/webview2/mod.rs)). The production adapter
will use profile-scoped WebView2 cookie managers with bounded native callback
reservations and deadlines. Microsoft documents that cookie-manager changes
apply to the user-profile context, `GetCookies` is URI-scoped, and
`AddOrUpdateCookie` applies a native cookie; the native cookie object exposes
`IsHttpOnly` for exact preservation
([CookieManager](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2cookiemanager),
[Cookie](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2cookie)).

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
- creates only a hidden, unfocused `about:blank` child with permissions,
  downloads, popups/page-close, media surfaces, autofill, link previews, and
  inspection denied;
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
The WebKit gate admits the canonical requested URL only: its construction-only
`about:blank` permit is one-shot, and unarmed page navigation plus redirects
remain denied until their policy contract is implemented. Commit and a
30-second watchdog share one atomic terminal claim, so exactly one can enqueue
the settlement. One navigation-or-recovery page-load terminal and one
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
pointer-identical ephemeral store, disabled inspection, and hidden state. A
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

## Deterministic evidence

```sh
cargo test --locked -p zephium-agentic
cargo clippy --locked -p zephium-agentic --all-targets -- -D warnings
cargo test --locked -p zephium-engine --features agentic-browser --lib
cargo clippy --locked -p zephium-engine --features agentic-browser \
  --all-targets -- -D warnings
cargo check --locked --target x86_64-pc-windows-msvc \
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
Profile-lease tests cover exact selection, kind compatibility, duplicate and
capacity refusal without eviction, durable/ephemeral storage retention, stale-
release rejection, deletion races, and shutdown quiescence.
Cookie-transfer tests cover canonical/deduplicated origins, Windows clean-
subprofile proof, handoff owner/profile joins, required capabilities, payload
and HttpOnly count invariants, explicit partial application, destination
single-flight, process concurrency, redacted debug output, and shutdown drain.
Handoff tests execute complete macOS shared-store and Windows cookie-bridge
flows through the real context registry, including exclusive human control,
temporary-context release, scoped refresh, fresh observation, incompatible
proof, out-of-scope commit, partial-transfer contamination, stale operation,
redaction, and exact cancellation resource dispositions.

## Remaining M2 work

1. add Windows owned construction with a truthful enabled-inventory proof,
   stable selected-profile/subprofile binding, and cleanup-debt ownership;
2. add bounded redirect/page-replacement observation, presentation, and
   suspension while preserving exact context/world/frame generations;
3. implement the bounded Windows cookie adapter and native borrowed/handoff
   transactions without changing ordinary extension principals;
4. qualify macOS construction/storage/inventory/close and both-platform
   lifecycle, idle-resource, cancellation, recovery, and shutdown behavior on
   explicitly authorized named devices.
