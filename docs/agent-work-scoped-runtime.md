# Scoped Work actor runtime closure

Status: production runtime foundation and read-only retained backing in the
existing Work controller, with deterministic actual-worker/loopback-provider
tests through the private application resource owner. Durable coordination and
product admission remain separate. No native, public-site or UI qualification
is added by this integration.

## The ownership distinction

Native lease delivery proves that exact resource-local revocation and its
physical callback/task delivery returned. It cannot prove that an actor stopped,
that a provider or audit obligation settled, or that a second actor is allowed.
A useful Work-owned page must survive actor closure without changing the
meaning of the accepted complete-browser lifecycle.

`PendingScopedAgentRuntime` therefore uses the **same** process-unique worker,
current-thread executor, mailbox, control queue, cancellation/deadline handling,
controller polling, terminal claim and original thread-join machinery as legacy
runs. A separate typed startup marker transfers no native port. The scoped entry
adapter receives only the same affine `AgentRuntimeWorker`; the trusted
controller supplies its narrow actor facade. The original Work owner, stable
native sink, resource registry, profile and destruction authority stay outside
the runtime. This is not a second controller algorithm or executor.

The binding borrows the original move-only approved manifest and freezes an
opaque non-authorizing settlement stamp: manifest identity plus its complete
immutable authority fingerprint. It also freezes the exact resource-incarnation
execution lease. Run, profile and original lease/manifest deadline intersection
must agree. The original manifest remains available for its real mutable policy.
No IDs, serialized record or fingerprint alone creates authority. This primitive
binding is not current native health, fresh account evidence, task admission or
permission to execute; the facade/native adapter still rechecks the original
monotonic execution deadline.

## Scoped closure protocol

The scoped worker calls the existing mailbox/control claim algorithm. It closes
ingress, waits for already-accepted writers, rejects queued terminal/signal or
control debt, and joins the exact run ticket and cancellation/shutdown class.
The ordinary claim API refuses scoped workers; a scoped claim refuses ordinary
workers. Dropping an uncommitted claim fail-closes that actor.

Scoped commit independently consumes:

- the exact lease's native retirement plus completed delivery proof;
- a consumed policy/metric/audit settlement matching the frozen full manifest;
- the trusted controller's original provider transport shutdown proof.

The provider proof keeps its existing contract: it is constructor-closed sealed
transport evidence, not a new serialized provider identity or accounting claim.
The controller must retain and supply its actual transport owner; this runtime
cut does not infer usage accounting from an unrelated idle transport. Mismatched
lease/manifest operands return losslessly with the uncommitted claim.

A commit still is not drain. `AgentRuntimeScopedLifecycle::drain_until` requires
normal controller-future return **and** destruction, fault-free closed mailbox,
the committed run state, actual original thread join and worker-permit release.
Post-claim polling/destructor panic clears retained closure. Timeout or lost
lifecycle ownership hands the original worker to the existing bounded reaper
and produces no scoped proof. The application resource owner remains available
for its independent reconciliation/destruction obligations.

`AgentRuntimeScopedDrained` is move-only, allocation/ticket/lease-bound evidence.
Numeric tickets can repeat across runtime allocations; they cannot cross-join
proofs or controls. The scoped outcome does not implement the complete-browser
lifecycle and cannot convert into native-zero proof. Legacy global Clean and
successor contracts remain unchanged and still require original native/resource
destruction and application closure.

## Evidence and remaining joins

Tests exercise the actual named worker with real functional resource
construction/acquisition/revocation/delivery, consumed policy/audit ledgers and
a real sealed idle provider transport. The transport has no attempts, requests
or credentials. These receipt-bearing integration tests live in the controller's
existing opt-in `provider-transport` test graph and use only public runtime APIs.
The runtime declares no transport feature or development dependency. Its default
unit tests exercise held callback ingress and queued-terminal refusal directly.
No empty legacy native registries are substituted for retained-resource proof.

The retained-controller join additionally needs account binding before a read
and a wake after physical native delivery. The original resource registry now
issues a move-only `WorkBrowserReadBinding` only for its current exact lease and
fixed document. It contains descriptive frame/lease coordinates; it reserves no
read or invocation and cannot enter legacy context dispatch. Each actual read
independently rechecks the row and uses the same correlation helper.

The delivery ticket optionally registers one immutable listener before native
dispatch. Registration never runs arbitrary wake code, even for a ready terminal.
Only the original move-only native notifier can run it. Notification progresses
from Pending to Running to Completed/Failed; Running is not completed delivery.
The registration window closes when that notifier starts, or when listener-free
publication occurs without a notifier. Later registration returns the distinct
fail-closed `RegistrationClosed` refusal. Registration after publication is only
accepted while its original notifier is still pending.

One coordination lock linearizes listener registration, notifier reservation and
publication. If registration wins, missing-notifier publication refuses; if
listener-free publication wins, registration refuses. There is no separate-load
window that can silently lose a required wake. Terminal consumption still uses
the original exact acquire-consuming compare/exchange. The native guard publishes
the physical fact without invoking code under its lock, then notifies outside
both locks while its exact retirement reservation blocks Acquire/destruction and
global drain through the actual listener return. A held or reentrant listener
cannot release that reservation; a late panic quarantines the exact resource.
Every invalidating registration refusal publishes failure while still holding
that same coordination lock, before the notifier can decide completion. Releasing
the rejected proposed waker and returning the refusal happen afterward, outside
the lock; neither is needed to make the failure visible to native retirement.

Notification has no receipt or execution authority. Its failure cannot replace
or retroactively rewrite a physical returned receipt. A future product successor
still needs current resource health and native admission, not just that immutable
receipt. Listener-free legacy polling and fixed request-size ceilings are unchanged.

Prerequisite regressions exercise binding without reserving an invocation,
foreign/stale/expired leases, publication both before and after registration,
duplicate/lost/poisoned/panicking listeners, closed registration, missing and early
notification, unproven terminals and single consumption. Threaded native-ledger
schedules hold the original listener through successful return, late panic and
late registration while checking unlocked coordination, exact Acquire refusal,
destruction debt and global/successor refusal. They also include the inverse
refusal-return schedule: the refused waker destructor holds
registration before external return while the original listener finishes and
native quarantine becomes visible. A physical receipt consumed during Running
remains unchanged. The locked refusal ordering is also mechanically pinned.
Concurrent missing-notifier races have present-notifier and listener-free
positive controls. This is
deterministic native-adapter evidence, not a new rendering or provider workflow.

Adversarial schedules cover post-claim blocking/deadline, polling panic,
future-destructor panic, lost lifecycle, lost/no claim, accepted callback held across a manually
polled Pending claim, queued terminal refusal, exact cancellation/shutdown class,
foreign lease, same-ID/different-manifest substitution, scope deadline/profile/run
mismatch, and allocation-specific proofs/old controls. The original functional
resource remains retained through actor failures and is destroyed only by its
original owner. This is not additional native retention evidence.

## Common controller over the private retained facade

`AgentWorkRetainedController` runs the existing `AgentWorkController::execute`,
`browser_loop`, `AgentBrowserSession`, provider pump, trusted account/task checks,
source-bound extraction and policy/metric/audit closure. Only the native backing
and terminal claim differ. The retained branch never constructs a legacy context,
profile, cookie or screenshot registry, and cannot dispatch legacy effects or
claim a native port seal. Its original session transport—not an unrelated idle
transport—supplies provider closure. An original physical delivery proof remains
retained if later session closure refuses.

The controller's closed `AgentWorkRetainedBrowser` port exposes original read
binding, immutable listener registration, exact health, one bounded initial read,
and lease revocation/delivery polling. The only production implementation is a
private application adapter around the original `LeaseBrowser`. Application-owned
operation slots, resource observer, native sink and destruction authority never
move into the runtime. Source correlation comes from the original core-admitted
request, not a new counter or registry. Optional model-selected baseline reads
reuse the acknowledged observation without additional native capture. Navigation,
actions and subtree requests remain excluded; task capability mutation refuses.

One immutable listener per exact lease is registered on the actual worker before
dispatch, including registration on the original delivery ticket before native
revocation. A bounded weak listener lane fans out scalar wakes while preserving
the stable application sink. Publication precedes wake; polling acquire-consumes
the coalescing signal and rearms the original health observer. Duplicate, poisoned,
lost-owner or panicking listener authority fails closed. Scalar publication first
sets the pending signal; concurrent or reentrant publications coalesce while it
is pending. If the worker rearms while a publisher's wake is still returning, a
new publication may invoke the thread-safe Waker concurrently. Overlap itself is
not a resource fault; each invocation still catches panic and fail-latches its
exact non-retired resource. The scalar signal is never a delivery receipt or a
native notification-return proof. Arbitrary wake and final Waker destruction
happen outside the owner lane/slot-collection locks.
Expired/stale A facades cannot read B; A's exact retired marker prevents a late
old wake or facade drop from poisoning B.

`AgentWorkRetainedOutcome::Accepted` is explicitly **non-durable**: the trusted
task accepted an exactly source-bound, schema-valid but ModelMapped result, not a
factual-verification or user-objective-success proof. The actual scoped worker
drain remains a separate operand. Recovery has no durable admission or global
native shutdown adapter. No public application admission API is added.

The retained error path always runs the common bounded recovery drain, including
when initial close already moved the original session, provider and lease receipt
into `WorkDrained`. Already-dispatched audit terminals are reconciled from their
original runtime queue or bounded deferred lane, with exact ledger/proof matching;
foreign and duplicate terminals stay retained. Cleanup neither reissues provider
or native work nor repeats supervisor completion/audit dispatch. A quiescent
original audit may proceed through the existing metric/policy/runtime claim using
the originally chosen supervisor outcome. A late cancellation that defeats that
ordinary claim remains Recovery/Unproven, even if its audit is now fully accounted;
lost delivery retains the original in-flight debt. Scoped recovery exposes only
content-free audit status, not reconciliation or admission authority.

Physical delivery may be consumed while the native notifier is Running. The
scoped worker can therefore drain before that notification returns; it proves
actor closure only. The original native reservation still blocks Acquire and
destruction, and a late notifier failure quarantines the resource. There is no
recursive second notification barrier. The adversarial actual-worker schedule
holds that original listener through actor drain and independently observes
native B/destruction refusal. This does not qualify their later success.

Deterministic application fixtures cover the actual two-call extraction and
three-call model-selected baseline-read loop; result retrieval before original
resource destruction; exact original scoped worker drain; foreign-source
rejection; lost audit; cancellation and lost/late read callbacks; idle native
health waking the worker; duplicate/poisoned/panicking listeners; truly concurrent
held wake/coalescing/rearm/late-panic schedules; reentrant scalar coalescing; queued
audit delivery after cancellation of initial close (with no-cancel and lost-delivery
controls); deferred foreign/duplicate audit isolation; and late-panic stale-A
isolation. Their native adapter is synthetic: original macOS retention,
rendering and full application/global weak closure remain the separate previously
qualified evidence, not claims of this loopback fixture.

Next cuts must join explicit durable scope, original Store acknowledgement,
result publication/retrieval, settled current resource health and fresh B
task/account/manifest admission. The existing v1 journal meanings, artifact
publication and legacy successor API are unchanged.

Integration regression checkpoint: nine focused private-application schedules,
592 + 3 + 5 core tests, 504 native-engine tests, 38 runtime tests, 41 controller
tests plus six scoped-runtime integrations, and 380 application tests (one
existing ignored test) pass. Strict all-target core/engine/controller/application
Clippy, all three architecture gates, resource-boundary mutation tests, hostile
semantic JavaScript smoke, default and `macos-work` release desktop builds,
formatting and default Browse provider/runtime dependency isolation pass. These
are deterministic/build results; no GUI, credential or public provider service
was used. All actual-runtime application fixtures share the existing process-wide
worker test lock; the production one-worker admission limit is unchanged.
