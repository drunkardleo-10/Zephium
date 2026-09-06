# Persistent Work browser resources and execution leases

Status: functional core, typed ports and opt-in macOS native integration;
**the native two-lease retention witness is not yet qualified**. Other adapters
remain explicitly Unsupported. Deterministic tests do not claim that a real
page has survived two actor leases. The existing run-owned qualification path
remains available, and its all-zero proof now also excludes retained Work
resources and their outstanding delivery owners.

## Why this boundary exists

The actual-application foreground witness establishes a bounded supported
rendering opportunity for one exact page. It does not solve ownership: current
`ContextIdentity` permanently owns a run, normal cancellation stops loading and
fails the isolated document channel, and current successful runs destroy every
native context. Reusing those mechanisms as persistent Work resource lifetimes
would either lose the useful page or silently change the meaning of Clean.

`WorkBrowserResources` owns one selected Work/profile's browser-resource
identities. `WorkId`, `WorkBrowserResourceId`, profile and `ContextId` are stable
resource coordinates containing no actor run. Only this non-executing identity
is serializable. Each process-local resource join additionally carries an
incarnation and private registry-allocation binding: reconstructing identical
durable IDs cannot accept another instance's request or receipt. Construction
does not itself attest durable Work/profile admission; the trusted application
must supply that original owner. No Store migration or restart resumption is
implemented at this checkpoint.

An independently run-bound `WorkBrowserExecutionLease` adds its exact run,
monotonic generation and original absolute deadline without relabeling the
resource or page. It is neither an account/effect/navigation capability nor a
model-facing tool. Future page operations must also join the approved policy,
account, current document, observation and operation-specific native permit.
This checkpoint enables only an explicit trusted construction source and the
existing bounded initial all-role observation, not general navigation or effects.

## Exact lifetime protocol

Construction, acquisition, revocation and destruction publish their pending
owner before returning a move-only native request. A completion consumes that
original request. The core checks exact resource/incarnation/private authority,
operation class and lease; an ordinary audit or another resource's zero counters
cannot substitute. These are trusted imperative-port facts, not cryptographic
proof: the native adapter must inspect its actual original owners before
reporting them.

Acquisition acknowledgement receives trusted monotonic time. An expired lease
or a shutdown seal yields `RevocationRequired`, never `Leased`; its same native
owner remains available for explicit revocation. The adapter must independently
check that original deadline at actual dispatch and completion. The Work-lifetime
clock domain is retained across leases, not rebased to each actor run. Clock regression
quarantines execution but does not drop an exact native terminal or prevent
physical cleanup. Native absence is not a clean clock, audit or task verdict.

Revocation seals execution admission before native dispatch. Only an exact
terminal with zero lease-owned task/observation/action/navigation/capture/callback
debt **and the same resource still retained** produces `WorkBrowserLeaseEnded`.
The page disappearing is not lease-only success. A new actor requires a fresh
explicit lease and all higher product/policy/account admission again. Old lease
coordinates remain stale; no lease or receipt is deserialized or automatically
replayed. Human takeover is not implemented. Quarantine or shutdown racing
acquisition cannot be undone by a late success.

The resource owns long-lived page/profile/isolated-world pull, location and
renderer channels. Per-invocation results, actions, captures and their callbacks
belong to the execution lease. Document-wide invocation ceilings must persist
across leases; acquisition must not reset counters, replace the isolated world,
reload the page, or call the legacy cancellation path.

## Native fixed-document slice

`construct_document` freezes the exact HTTP(S) source in the original move-only
construction request. Parsing a URL alone is not admission. The trusted caller
must already own Work/profile/source authority; no model-facing operation mints
this request. The macOS owner uses the existing extension-free selected-profile
constructor, content-policy registration and shared native resource ledger.
It first completes the one native `about:blank` bootstrap, then loads the frozen
source once after publishing its resource owner and policy. Exact native
start/commit/finish identities and the current URL must agree. Redirects,
reloads, unsolicited destinations and observed post-ready location changes
cannot authorize a replacement document. The empty-source construction path is
retained but cannot issue an observation.

The page lives in a separate resource map; native callbacks capture only its
stable private incarnation guard. They never capture the first actor's run and
then switch to a successor. A temporary `ContextJoin` exists only inside each
accepted semantic invocation/result correlation, with the actual run and fresh
lease generation. It cannot resolve through the legacy native-context map or
authorize navigation, actions, cookies, account access or cancellation.

Each lease can request one outstanding initial all-role/default-budget read.
Both core and native resource preserve a monotonic invocation sequence across
leases, including rejected admissions; the existing isolated-world document
ceiling is not reset. The original lease, document, frame, invocation and
snapshot-generation correlation binds the result. Expiry, revocation,
destruction or shutdown discards page-derived contents while accounting the
exact move-only terminal. This does not assert page semantic completeness.

`work_browser_monotonic_now` supplies the single process-monotonic domain to the
trusted caller. Admission, actual native execution and completion recheck the
original deadline; callers must not supply a per-run rebased clock. Construction
has a 30-second native bound, each read is bounded by the lesser of the remaining
lease and 15 seconds, and revocation/destruction each have a five-second cleanup
bound. Exact operation/lease-bound cancelled timers cannot expire a successor.
These are maximum lifetimes, not readiness delays or retry schedules.

Revocation synchronously seals ingress before main-queue dispatch. A queued read
is rechecked at execution and cannot run under the retired lease. Native read
results retain their original task permit through a mandatory next-main-queue
barrier after the WebKit callback returns, then through delivery to the core.
No clean revocation calls stop-loading, document failure, nonce rotation or
legacy semantic cancellation. Long-lived idle pull/location/renderer channels
stay resource-owned. Coalesced resource notifications share the original native
task queue ceiling; they are not a new unbounded queue.

The lifecycle terminal itself has move-only transfer semantics: at invocation
of its `FnOnce` receiver the exact request/receipt is application-owned, so the
native resource no longer owes that lease callback. The shared native delivery
permit remains held until the receiver returns. Destruction also retains its
ingress row through that return. Thus a receiver may account `LeaseEnded` and
request another lease, but cannot produce reentrant global-zero or successor
proof while native delivery is still active. Read callbacks, unlike this
ownership-transfer terminal, retain explicit lease callback debt through return.

## Failure, cleanup and bounds

Uncertain/refused/malformed native results quarantine only the exact resource.
Quarantine retains callback and execution-capacity debt; unrelated resources
remain usable within the capacity ceiling. It cannot unquarantine itself after
a late receipt or create a replacement empty cohort. Engine-global faults and
whole-process shutdown remain separate authorities.

One independent destruction slot remains available for a quarantined resource
even if its original callback is lost. Destruction must attest actual page,
resource-owned profile lease and resource-channel closure, not deletion of
profile data. It cannot erase the earlier callback:
destruction-before-callback and callback-before-destruction converge without
resurrecting the lease. Runtime bookkeeping can be reaped only after both exact
obligations settle. A rejected or unproven destruction remains quarantined; no
blind retry occurs. Cleanup operation identities are reserved by their resource
or lease, so exhausting acquisition identities cannot prevent cleanup.

Port non-admission is lossless: an Unsupported/rejected adapter returns the
original request and transfers no callback obligation. The application accounts
that typed refusal rather than inventing a native completion. A never-admitted
construction proves absence; a never-admitted acquisition preserves the prior
retained resource. Rejected revocation/destruction does not erase the existing
native owner. Non-macOS adapters use the default Unsupported path.

Native partial-construction uncertainty retains the exact resource reservation
and quarantines that resource; a fallible constructor returning an error is not
proof that its native delegate/view graph vanished. Known construction refusal
before any native allocation can still prove absence through exact destruction.
Neither case invents a clean global shutdown. An idle renderer/location failure
closes native admission; the next operation refuses. A separate asynchronous
product resource-event stream is not implemented at this checkpoint.

The registry retains at most the existing eight resource rows and four execution
reservations, counting quarantined leases until exact destruction. A row holds
one ordinary operation and one independently reserved cleanup operation; no
unbounded history, timer, worker, script, page content or provider dependency is
added. A native adapter must enforce these same shared process ceilings across
Work registries and the legacy path, not sum independent per-Work allowances.
The native map and ingress enforce shared process bounds. When mixing with the
legacy path, every retained legacy context conservatively counts as an execution
reservation; no independent per-Work allowance is added. Quarantined native
reservations remain counted. Policy replacement, profile-erasure barriers,
native audits, forced shutdown and legacy successor admission include these
resources. Destroying a page releases its selected-profile lease, not the user's
profile data; the existing tombstone refuses revival after erasure admission.
No global native proof may ignore retained resources. Local `is_quiescent` is
not global native shutdown, policy/provider/audit drain, durable terminal
acknowledgement or a successful task. Default Browse remains dormant.

## Next integration proof

In the ordinary application lifecycle, a provider-free witness must
exercise two distinct read-only leases over one unchanged native page, reject
old lease/observation authority, preserve the document invocation ceiling, and
eventually prove exact resource destruction and full application shutdown.
The rendering probe remains a qualification tool, not a product capability.
The production resource has no rendering-presentation permission. Its witness
must use the independently qualified, release-excluded foreground holder and
claim only page retention under that holder, not hidden-page rendering viability.
Presentation, input takeover, persistent Store ownership, general task planning
and open-objective provider qualification remain separate unimplemented joins.

Previous functional-core checkpoint gates: 17 focused resource/lease schedules, all 511 core
`probe-harness` library tests and 3 + 5 evidence-review tests, all 41 controller
harness tests, 465 ordinary agentic engine tests, strict all-target core,
controller and engine Clippy, both resource architecture adversaries, both
architecture commands with hostile semantic JavaScript smoke, workspace fmt and
diff checks, the opt-in `macos-work` desktop composition check, and default
Browse dependency isolation. These are deterministic
contract/regression results, not native resource-retention qualification.

Native integration checkpoint (2026-09-06): 23 focused core resource/read
schedules, 14 native ingress/ownership schedules and three frozen-document gate
schedules pass. The full core suite passes 517 + 3 + 5 tests, ordinary agentic
engine 482, existing foreground-feature engine 523, controller 41, and
application 347 (one existing ignored test). Strict
all-target core/controller/ordinary-engine/foreground-engine Clippy, the
`macos-work` desktop composition check, both architecture commands and hostile
semantic JavaScript smoke, resource architecture mutations, workspace fmt/diff
checks and default Browse dependency isolation pass. One earlier controller
run hit its unchanged 12-second fixture deadline before any native call during
concurrent compilation; the isolated schedule and subsequent full suite passed
without code or deadline changes. The precise transient cause is not proven.
This record is build/deterministic evidence only; it contains no native
two-lease, real-site, provider, product rendering or human-takeover claim.
