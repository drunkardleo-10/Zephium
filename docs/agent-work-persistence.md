# Work persistence boundary

The optional application Work coordinator now owns admission and durable
recovery classification. This is not executable session restoration and does
not reopen the engine's single-use agent port.

The optional `zephium-store/work-execution` adapter uses the existing Store
actor and a lazy four-operation admission counter. It starts no worker, timer,
provider or browser. Default Store dependencies do not include the private
filesystem adapter. Meta migration 17 adds empty, bounded Work tables; no Work
namespace or lock is created until explicit claim.

A claim acquires the existing `LockedPrivateNamespace` authority under the
application data directory. Store mints a fresh incarnation itself; callers
cannot restore an old incarnation. The exclusive lease remains held until OS
process exit, including after uncertain transactions and Store shutdown. A
bounded process-static owner prevents replacing a Store from reopening Work
admission while an unclean native engine teardown may still be running. Every operation
revalidates that exact lease. This currently admits macOS/Linux; unsupported
platforms and transient in-memory stores fail closed.

Within one FULL-synchronous SQLite transaction, claim classifies all prior
nonterminal records as interrupted and publishes the new process fence.
Already-terminal records remain byte-for-byte unchanged. A crash, missing
acknowledgement or partial transaction never implies a native mutation did not
occur. Recovered facts cannot recreate refs, native contexts, provider state,
task predicates or executable approvals.

The retained-resource coordinator pauses in `NeedsReview` when claimed history
contains an `Interrupted` record under the new process fence. Its bounded handle
accepts an explicit review of those exact bytes; only the original Store CAS ACK
can classify that record as `FreshAdmissionRequired` or `Rejected`. Both retain
all historical debt. After every interrupted record is reviewed, a separately
prepared objective may acquire the newly constructed native resource and pass
ordinary fresh admission. Historical classification never settles old effects,
reconstructs authority, or reuses an old execution owner. Unreviewed records,
foreign nonterminal fences and unknown-debt `FailedClosed` history still block.
Cancellation, malformed ACKs and uncertain persistence keep admission closed.

For the release-excluded developer runner, set `ZEPHIUM_WORK_REVIEW` to a review
sidecar path. The runner reports exact content-free interrupted record hex while
waiting. An explicit JSON decision `{ "record": "<reported hex>", "decision":
"accept_fresh_admission" }` (or `"reject"`) submits that one review. No file means
no review; stale process/revision bytes cannot match. This is development input
to the same handle, not a model tool or an alternate persistence path.

Each fixed 96-byte record contains only version/disposition/debt bits, a checked
monotonic revision, process identity, manifest/run identity and the existing
content-free manifest guard. No objective, credential, page/provider content,
URL, profile path, screenshot or raw trace is stored. Debug omits identifiers.
The 1,024-run ceiling never silently evicts audit or recovery obligations.

Updates compare exact previous bytes, not just counters. An exact retransmission
can acknowledge an already-committed write, but cannot execute anything. Terminal
facts are immutable in both the core transition grammar and SQLite. Decoding a
fact does not mint a successful terminal mutation: its constructor requires the
original successful policy/audit settlement and native shutdown proof, joined
to the exact manifest guard. The application must additionally retain the
matching clean runtime lifecycle result.

The same constructor-closed boundary now admits distinct immutable `Failed`
and `Cancelled` facts with zero debt when the original policy closure reports
that outcome and the native/runtime owners prove drain. Generic transitions,
review decisions and decoded historical records cannot construct those writes.
`FailedClosed` remains a conservative classification, never itself proof of
clean cleanup; it preserves the prior debt, including a proof-closed review's
zero debt. The fixed 96-byte version-one
envelope adds two closed discriminants; older readers reject these unknown
values rather than guessing success. No schema migration or result body is
needed. Exact CAS, rollback, lost-ACK reconciliation and restart immutability
apply equally to all proof-bearing terminal outcomes.

Accepting review produces `FreshAdmissionRequired`; rejecting it produces
`Rejected`. Both preserve the exact prior execution debt. Neither resumes the original
proposal, and neither means clean resource settlement. Stale/conflicting
decisions fail exact CAS; a new user decision cannot reopen a terminal record.
Any later approved execution needs newly admitted native authority and a fresh
trusted observation.

An exact never-dispatched policy refusal can now use original failed
policy/audit/native/runtime closure to persist zero-debt `NeedsApproval`.
After an exact review decision ACK, `Reviewed` is eligible for explicit fresh
admission only alongside those retained original owners. Unresolved debt and
decoded historical facts cannot qualify. Unreviewed records still become
`Interrupted` with unknown debt on restart. See [closed review](agent-work-review.md).

## Application admission and reconciliation

`zephium-app/work-execution` provides `CallbackHandle::attach_work`,
`PreparedAgentWork` and the content-free `AgentWorkApplicationHandle`. The
composition root supplies the same `Arc<SqliteStore>` allocation for ordinary
Store, Work journal and audit ports, and the same engine owner for ordinary
Shell, attachment and prepared execution. The shell rejects a different owner,
an ordinary second coordinator, or an already-owned legacy agent lifecycle. Attachment
claims the durable recovery inventory, but creates no runtime or native page.

Preparation owns the existing stateless `AgentWorkController`, trusted manifest,
task predicate and provider configuration. The one-shot native factory must
transfer a port from the actual engine's unique native authority; it is not a reset or
reopen operation. Only after exact durable `Admitted` and `Running`
acknowledgements does the shell create the original runtime, invoke that factory
and start the controller. Original deadlines include all persistence waits.

An explicit `attach_successor_work` may replace only the exact previous fully
closed coordinator, joining original lifecycle/native/policy/audit owners to
its acknowledged immutable terminal. Pending outputs/events or any uncertain
owner refuse replacement. The same engine/Store and process fence persist;
the new projection/record/runtime/native lifetime are distinct. See
[sequential lifetimes](agent-work-lifetimes.md). Terminal facts alone, including
decoded historical facts, never authorize this replacement.

The shell retains the runtime lifecycle and move-only outcome. `Succeeded` is
published only after the original controller policy/audit closure, native proof,
clean runtime worker join and durable terminal CAS acknowledgement. A timeout,
refusal or lost callback never substitutes empty owners. Even an undispatched
cancelled controller clears credentials/objective and retains its original
non-executing recovery owner; draining audit alone cannot establish Clean.
If a bounded unclean lifecycle join precedes the worker's final Drop, the
completion wake still transfers that original late recovery outcome into the
application. The earlier unclean result remains unclean.

A fully settled unsuccessful actor outcome uses the same lifecycle/native
join, publishing `Failed` or `Cancelled` only after its exact immutable terminal
write is acknowledged. A lost ACK retains that original CAS for bounded
explicit reconciliation; a late cancellation cannot change its disposition.
No result is released or artifact published for this outcome. Historical
artifact reads are independent of the current run's terminal phase.

The application uses one durable request slot and one recovery audit slot, each
with a two-second acknowledgement deadline and at most four explicit
reconciliation dispatches. Reconciliation can retransmit only the identical
retained CAS or the exact original audit ledger delivery. A newly prepared audit
batch uses the ledger's checked next delivery identity, not a guessed count.
No provider request or native action is replayed. Undelivered audit remains debt;
draining it does not clear native, provider, accounting or lifecycle obligations.

Explicit durable-profile extraction can require an atomic private
[artifact/terminal publication](agent-work-artifacts.md). It shares the same
retained request slot and Store permits, not the content-free journal/audit
body format. Read-only archive failures release their own slot without consuming
future admission; publication uncertainty retains the exact original owner.

The existing shell mailbox carries one globally coalesced Work wake. No extra
worker or periodic poll is added; the existing timer wakes only for pending
acknowledgement/admission deadlines. The application projection holds at most 64
events plus one retained overflow event, in addition to the controller's original
64-event queue (129 total). Pressure revokes execution without overwriting events;
consumption wakes the shell to drain retained events. Typed state and recovery
inventory contain no objective, page/model content, secrets or native handles.

Review is an exact incarnation/revision CAS, not a model tool approval. Accept
records `FreshAdmissionRequired`; reject records `Rejected`. Cancellation and
review are ordered by the first retained CAS. A replay, stale revision or foreign
application handle cannot admit work. Each coordinator and native port admits
one execution attempt. Fresh approved execution requires an explicitly joined,
fully closed predecessor and a distinct native lifetime, not continuation on
the consumed coordinator or release of the process fence. Restart never recovers a
credential, task predicate, observation/ref or mutation permission.

Deterministic tests cover the complete disposition-pair grammar, malformed
records, overflow, live-owner exclusion, restart classification, partial restart
and write rollback, lost commit acknowledgements, approval/reject/cancel races,
terminal immutability, retention pressure, callback loss/panic and bounded Store
mailbox admission. A real subprocess fixture additionally proves exclusion
before and after Store drop, and lock release after actual child-process exit.
Unit restart injection releases only a test-owned fence with no live Hub;
production has no fence-release operation. Application tests additionally drive
the actual public shell command path with the same SQLite Store for ordinary
storage, journal and audit, using a deterministic typed native fixture. The
fault matrix covers approval acceptance/rejection/replay, cancellation/takeover,
lost native callbacks, uncertain admission/terminal acknowledgements, runtime and
native-factory refusal, bounded wake pressure and exact audit redelivery.

The optional [macOS composition adapter](agent-work-composition.md) now supplies
the actual deferred EngineHost factory and explicit trusted task contract. Two
public Luna runs through that adapter, actual shell and native engine completed
with durable success and clean shutdown; the [M6 record](../eval/agentic-browsing/m6-production-qualification.md)
separates that evidence from full desktop UI/bootstrap qualification. Remaining
seams are trusted product task/plan authoring and user-facing Work state;
persisted facts deliberately cannot automatically resume execution. No new UI,
site tools, platform suspend support or battery qualification is included.
