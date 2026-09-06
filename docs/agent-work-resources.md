# Persistent Work browser resources and execution leases

Status: functional core, typed ports and opt-in macOS native integration;
**one native two-lease retention witness is qualified** under the separately
qualified, release-excluded foreground holder. Other adapters remain explicitly
Unsupported. This proves one fixed native document survived two actor leases,
not product rendering authority or general task execution. The existing
run-owned qualification path remains available, and its all-zero proof now also
excludes retained Work resources and their outstanding delivery owners.

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

### Stable native health (deterministic integration)

Tracked construction can install one move-only, private-resource/incarnation-bound
health reporter with the original native guard. Its one application receiver and
immutable registered wake outlive leases; no actor mailbox or first-run closure
is retained or rebound. The finite sticky states are Pending, Current, Retired
and Uncertain. Publication precedes a coalesced wake. Missing registration,
foreign installation, duplicate installation/registration, receiver loss, poison
and wake panic fail closed; Uncertain cannot heal. Retired describes the reporting
owner's retirement, not native absence or global shutdown proof.

Native acquisition, execution and successful lease-delivery settlement independently
recheck this exact health binding. Native uncertainty publishes outside ingress
and resource-state locks. A tracked resource retains one counted reporting lane
within the existing native task ceiling (at most `MAX_LIVE_CONTEXTS` such lanes);
this intentionally reduces available task slots, without a ceiling increase or
new thread/timer. The reporter drops before that lane's permit. Deterministic
destruction evidence checks callback return, operation-permit release, ingress
removal, then a Retired wake that still observes the reporting permit before its
release. Rejected construction breaks its original admission/guard ownership
cycle without native allocation. Untracked legacy qualification remains unchanged.

Health polling consumes the pending signal with an acquire/release exchange,
not a release-only store. This acquires a coalesced publisher's earlier sticky
state update; publication after the exchange instead reserves the next wake.
Both fixed linearizations and bounded concurrent races are tested, with the
acquire-consuming operation independently pinned by the mutation gate.

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
of its `FnOnce` receiver the exact request/receipt is application-owned. This
does **not** prove physical callback delivery has finished. Native revocation
retains an exact lease-bound delivery reservation and the original shared task
permit until the receiver returns. Even the legacy, untracked revoke path
cannot admit or execute another native Acquire during that interval. Read
callbacks retain explicit lease callback debt through return. Destruction also
waits for revocation delivery and retains its own ingress row through return.

`revoke_with_delivery` additionally creates one fixed, move-only poll/consume
ticket. The native adapter takes its completion half from the original request;
only after normal callback return, exact task-permit release and a healthy
retained-resource recheck can it publish. Publication is a single atomic-slot
transition under the resource admission guard, not a second callback requiring
another delivery proof. Polling is nonblocking; the eventual application owner
must use bounded wake-driven polling outside the revocation callback, not spin
or synchronously wait there. An unhandled/dropped completion publishes an
unproven result. Consumer loss before publication, callback panic or uncertain
task release cannot reopen native execution. Discarding an already-published
ticket grants no product successor authority either. The original resource
remains available for explicit cleanup; a cancelled wait cannot erase an active
callback obligation.

The ticket can be consumed once. Joining its receipt with `LeaseEnded` checks
the exact lease and private request-allocation binding; foreign, stale,
untracked or unproven receipts fail losslessly. The resulting
`WorkBrowserLeaseDeliveryProof` means only exact lease retirement plus physical
revocation-delivery drain. It is not current resource health, worker/run closure,
durable terminal acknowledgement, task success or global native shutdown, and
has no conversion to those authorities. No controller/runtime or global
lifecycle enum is changed by this proof cut.

The core may reach `Retained` at `LeaseEnded`; **that is not product run-B
admission**. An early logical Acquire is losslessly refused by native ingress;
the caller must account its returned exact request to restore core `Retained`.
The eventual product join must require delivery proof and independently scoped
run/worker closure, durable terminal acknowledgement, fresh task/policy/account
admission and current resource health before another actor. Those joins remain
unimplemented. A retained page still refuses global-zero and legacy successor
proof even after its delivery ticket is consumed successfully.

Construction admission publishes an outstanding-constructor obligation before
calling the main-thread dispatcher. Destruction seals that exact resource's
construction phase synchronously; both host entry and native construction
recheck it. FIFO dispatch alone is insufficient: concurrent port callers may
publish Construct, then deliver Destroy to the host before Construct is queued.
An absent host row in that schedule is not an absence proof. Destroy retains one
bounded, no-view cleanup reservation in the existing native resource pool until
the original constructor and its callback barrier drain. Stale Construct cannot
create a view, start a load or allocate an additional native reservation. If the
constructor is already retained by the host, destruction first settles that
original task with Refused, then waits for its barrier; it must not wait for an
obligation whose terminal it still owns. No sleep or new capacity is added.

The same ordering applies to a host-retained Revoke when explicit destruction
overtakes it. Destruction first refuses that original Revoke, then tests the
physical drain predicate. Otherwise it would wait for a delivery reservation
whose terminal it still held. The refusal is not `LeaseEnded` and a tracked
ticket is unproven. Accepted reads, the original callback-return/task-permit
barrier and the deferred resource notification still have to drain before
Destroyed. Taking the host-owned terminal cannot bypass any of those owners.

A synchronous Construct non-admission uses the same ingress mutex as Destroy
admission. It removes ingress only when no destruction owner has been admitted;
otherwise it drains the original construction obligation and wakes the retained
cleanup owner. A refusal or lost wake cannot erase that owner or reopen native
construction. If the existing cleanup reservation or five-second drain bound
cannot be satisfied, destruction refuses and retains uncertainty, not a false
Destroyed receipt or a replacement empty cohort.

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

## Qualified native boundary and remaining integration

In the ordinary application lifecycle, one provider-free witness now exercises
two distinct read-only leases over one unchanged native page, rejects old
lease/observation authority, preserves the document invocation ceiling, and
proves exact resource destruction and full application shutdown.
The rendering probe remains a qualification tool, not a product capability.
The production resource has no rendering-presentation permission. The witness
uses the independently qualified, release-excluded foreground holder and claims
only page retention under that holder, not hidden-page rendering viability.
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

Construction/destruction ordering correction (2026-09-06): independent review
found that a delayed constructor could outlive an overtaking destruction's
absence receipt. The adapter now retains and rechecks that original obligation
as described above. All 18 focused native ingress/ownership schedules, 23 core
resource/read schedules, 486 ordinary engine tests and 527 foreground-feature
engine tests pass, together with strict all-target Clippy for both engine
configurations, the `macos-work` desktop check, both architecture commands and
hostile semantic JavaScript smoke, resource/foreground architecture mutations,
fmt/diff checks and default Browse dependency isolation. The new deterministic
ownership tests use actual move-only task delivery, the native resource ledger
and an explicitly held callback barrier: no view or additional construction
reservation, no early Destroyed event, and exact final original-owner zero are
asserted. They cover overtaking construction, host-retained construction and
synchronous non-admission; they do not execute a real AppKit page or qualify
the then-pending two-lease witness. No GUI or provider run accompanied this fix.

### Physical revocation-delivery proof cut (2026-09-06)

The tracked poll/consume barrier and native admission closure described above
pass 30 focused core resource/read/delivery schedules and 24 native ownership
schedules. New adversaries hold the real callback after enqueueing `LeaseEnded`,
attempt reentrant and concurrent early Acquire, panic after terminal transfer,
discard an accepted task or waiting ticket, and inject expired/cancelled waits,
port sealing, resource quarantine and uncertain task release. Exact private-slot
substitution, stale-lease cross-joins, unhandled adapters, synchronous refusal
and double consumption cannot establish delivery proof. The untracked legacy
revoke has the same physical callback-return barrier. The tests directly assert
that retained resources refuse global shutdown verification and legacy
succession, including after an exact delivery proof has been consumed.

Full regression gates pass: core `probe-harness` 524 + 3 + 5; ordinary agentic
engine 492; resource/foreground-feature engine 539; strict all-target core and
both engine configurations; both architecture commands and hostile semantic
JavaScript smoke; two resource and six foreground architecture mutation tests;
workspace fmt/diff checks and default Browse dependency isolation. Eight
loopback fixture tests initially hit sandbox socket-permission errors; the same
full core suite passed with loopback permission, without code changes. These
are deterministic/build checks only. No GUI/native witness or provider run
accompanied this proof cut, and the earlier qualified witness retains its own
pinned source and evidence boundary. Scoped run/worker/durable closure and
product B admission are still subsequent integration work.

Independent review of this proof cut found a destruction self-wait: the host
tested `retirement_delivery` absence before settling its own retained Revoke.
The corrective host ordering above is covered by tracked and legacy schedules
using the real move-only lifecycle/read tasks and native resource ledger, with
no native view. Both read-before-wake and wake-before-read orders retain the
exact destruction owner, refuse early global/successor proof, return an unproven
tracked ticket, and finish original resource destruction followed by exact
original-port shutdown only after callback debt and ledger reservations reach
zero. No callback, queue, deadline, authority or cleanup predicate is weakened.
Correction gates pass: 30 focused core schedules, 26 native/host Work-resource
tests, full ordinary/resource-feature engine suites of 494/541, strict all-target
Clippy for both engine configurations, both architecture commands with hostile
semantic JavaScript smoke, resource-boundary mutations and workspace fmt/diff.
This is provider-free deterministic evidence; no GUI or native app rerun occurred.

## Private application ownership cut (deterministic only)

`zephium-app::work_resources` now contains a private, dormant Work owner above
actor leases. It creates the native port once with an immutable Work-lifetime
sink, retains the exact functional registry and resource observers, and never
captures or rebinds an actor/runtime mailbox. Resource failures remain sticky
and local. The two-slot original global-audit lane is separate; unexpected
legacy events, overflow, poisoned state, lost owner or failed application wake
close admission. Consumers drain the global lane until Empty, acquire-consume
its pending signal, drain/re-arm resource observers and recheck for raced events.
Poison of the original resource map or functional registry closes every existing
facade, including facades that do not traverse that map. Poison of one resource's
health, facade, slots collection or exact pending operation instead latches only
that resource's failure. Lock-error handling never invokes the application wake
under an owner mutex. Poison is not repaired or treated as callback absence.
Observer rearming continues across resource-local errors before reporting
uncertainty, so a poisoned early map entry cannot suppress a later resource's
idle-failure wake. Raced global terminals remain intact and take delivery
priority; sticky local uncertainty is reported on the next empty poll.

The move-only actor facade exposes only exact lease/deadline-bound health,
the existing bounded initial read, and cleanup revocation. It cannot extract
the port or registry, construct/destroy/seal resources, navigate, act, transfer
cookies or replace the wake/sink. Duplicate facade minting for a lease is refused.
Dropping a facade before its own exact joined revocation marks that resource
unavailable; an independently completed A marker cannot poison B when stale A
is later dropped. Active-to-Failed and Active-to-Retired share one atomic
transition: a delayed failure publication blocks retirement/reuse, while a
retirement winner makes old actor failures and duplicate registration inert.
Explicit destruction and original global-shutdown audit remain
with the Work owner. Dropping that owner immediately closes new actor authority
without pretending to perform cleanup.

Each resource retains at most three exact operation slots: lifecycle, bounded
read, and an overtaking destruction. Every slot owns its one terminal receiver,
original refusal request, and any revocation ticket/terminal; actor handles only
poll that original slot. Native callbacks publish into their immutable one-slot
destination before waking. Dropping an actor handle cannot discard a late A
terminal: the owner drains it, including after destruction, without offering its
contents to B. Synchronous non-admission requires an already-disconnected callback
sender with no queued terminal; a retained or synchronously delivered callback
is an explicit contract failure whose debt remains recoverable. Native refusal
classes are preserved.
For a synchronously returned Construct, the never-installed native health
reporter is detached and retired immediately after dispatch returns, outside
every application mutex, before storing or settling the refused request. Its
unrestricted synchronous application wake may reenter original-owner draining;
later normal, abandoned or error-path slot cleanup contains no waking reporter.
Finished-slot inspection does not wait for an operation mutex while holding the
slots collection. An operation poll may independently inspect that collection's
poison state without creating the inverse lock-order dependency.

Read terminals are accounted before the core's zero-read revocation settlement.
After an exact terminal and delivery receipt are consumed, failed health/delivery
proof marks uncertainty but does not retain fictitious callback debt. Actual
missing terminals stay owed. The private primitive acquisition gate requires
healthy retained resource state, no retained operation debt and exact prior
delivery proof. **It is not product B admission or scoped run completion.** Local
resource retirement is separately named and cannot become a global native proof;
global notification failure still refuses it. Proven-absent construction can be
reaped without allocating or destroying a native page.
Abandoned draining reports local slot poison and retains its unsettled debt,
while continuing to account unaffected resources' terminals. A poisoned slot
does not manufacture a completed delivery, release a flight or admit a successor.

Deterministic tests cover two exact leases under one original owner/sink,
stale/foreign authority, idle failures, lost facade/owner, abandoned late reads
and revocations, destruction overtaking reads, unproven delivery, queued and
retained callbacks contradicting non-admission, wake rearming/overflow/failure,
reentrant refused-reporter wake across normal/abandoned/poisoned cleanup,
global versus resource-local poison with existing facades, late terminals in
poisoned read/revocation slots, nonblocking busy-slot inspection, and exact local
retirement. No GUI, provider, public site, result artifact or
native rendering claim is added by these fixtures. The existing controller,
runtime, journal, Store layout, lifecycle enums and legacy successor are unchanged.

The [scoped runtime foundation](agent-work-scoped-runtime.md) now adds a separate
actual-worker drain proof without moving this owner's native port or changing
global Clean. Its read-only common-controller backing now connects to this
private owner through an exact per-lease listener and original operation slots;
see the linked scoped-runtime contract for the non-durable result and evidence
boundary. Next joins remain separately reviewable: versioned durable scope and
original Store ACK; result publication/retrieval; and fresh task/manifest/account
admission for B.
ModelMapped extraction validity is not user-objective success. The eventual
end-to-end proof must retrieve accepted artifacts before resource destruction,
then join original destruction and global application closure afterward.

## Actual-application retention witness

The compile-time-only `macos-work-resource-probe` selects the resource driver
inside the existing isolated debug rendering bundle. The unchanged exact
foreground admission precedes the loopback fixture and Work allocation. A
private diagnostic transport shares the original native port's task permits;
it does not add a presentation method to the production `AgentBrowserPort`.
The renderer's closed owner key distinguishes a legacy context from an exact
Work resource incarnation. Its native surface and watchdog are retained by
that resource and included in native visibility, semantic admission/results,
destruction and shutdown. No product rendering owner exists without this
release-excluded feature.

The fixed sequence is construction and rendering, lease A and a bounded
complete observation, revocation with exact rejection/accounting of an original
stale A read, lease B and a fresh observation, revocation, rendering retirement,
resource destruction, original-port seal, and normal application shutdown.
Both actor runs and execution leases must differ while the resource stays the
same. Private in-memory stamps compare the native view, actual navigation ID,
isolated world and document-wide completed-invocation counter. The second stamp
must show exactly one additional native invocation; the stale refused read
cannot increase that native counter. Stamps are not serialized, logged or
offered as model authority. Exact fixture markers and current-document
completeness are checked under both leases.

The original five-second rendering opportunity, eight semantic-read ceiling,
15-second driver bound, four-slot mailbox and five-second cleanup bound remain.
The driver reserves one read for lease B; bounded readiness samples consume the
remaining seven, not an increased ceiling. Its final outcome can qualify only
after both exact `LeaseEnded` receipts, stale rejection, native identity/counter
continuity, resource-core quiescence, original native-cohort zero, unchanged human
ownership, fixture cleanup, normal application shutdown and exact native weak
drain. See `eval/agentic-browsing/macos-work-resource-retention.md` for the evidence
boundary and result. This is a provider-free ownership witness, not product
presentation, open-objective task, authenticated session or restart durability.

Reviewed native result (2026-09-06): the pinned source and executable completed
one actual-application run with two distinct leases and two exact lease-ended
receipts over the unchanged page/document/world. Both current-document samples
passed the fixed readiness markers, including animation-frame reveal; the
native completed-invocation count advanced from 1 to 2, while stale A authority
was rejected by both core and native adapter. Resource-core quiescence, original
native-cohort zero, unchanged human ownership, fixture closure, normal
application shutdown and exact native weak drain all passed. The final result
is Qualified within the linked evidence boundary; no production authority is
added by that result.
