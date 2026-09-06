# Scoped Work actor runtime closure

Status: production runtime foundation with deterministic actual-worker tests;
not yet connected to the retained-resource controller, durable coordinator or
product admission. No native, provider-request, public-site or UI qualification
is added by this cut.

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
destruction debt and global/successor refusal. Concurrent missing-notifier races
also have present-notifier and listener-free positive controls. This is
deterministic native-adapter evidence, not a new rendering or provider workflow.

Adversarial schedules cover post-claim blocking/deadline, polling panic,
future-destructor panic, lost lifecycle, lost/no claim, accepted callback held across a manually
polled Pending claim, queued terminal refusal, exact cancellation/shutdown class,
foreign lease, same-ID/different-manifest substitution, scope deadline/profile/run
mismatch, and allocation-specific proofs/old controls. The original functional
resource remains retained through actor failures and is destroyed only by its
original owner. This is not additional native retention evidence.

Next cuts must connect the existing controller to the private retained-resource
facade, then join explicit durable scope, original Store acknowledgement,
independent result/provenance acceptance, settled delivery/health and fresh B
task/account/manifest admission. ModelMapped artifact validity is not objective
success, and neither this proof nor native retirement permits B by itself.
The existing v1 journal meanings, artifact publication and legacy successor API
are not changed here.
