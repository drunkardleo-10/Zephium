# Work persistence boundary

This is the durable prerequisite for application Work admission, not executable
session restoration. It does not reopen the engine's single-use agent port.

The optional `zephium-store/work-execution` adapter uses the existing Store
actor and a lazy four-operation admission counter. It starts no worker, timer,
provider or browser. Default Store dependencies do not include the private
filesystem adapter. Meta migration 17 adds empty, bounded Work tables; no Work
namespace or lock is created until explicit claim.

A claim acquires the existing `LockedPrivateNamespace` authority under the
application data directory. Store mints a fresh incarnation itself; callers
cannot restore an old incarnation. The exclusive lease remains held until the
Store owner closes, including after uncertain transactions. Every operation
revalidates that exact lease. This currently admits macOS/Linux; unsupported
platforms and transient in-memory stores fail closed.

Within one FULL-synchronous SQLite transaction, claim classifies all prior
nonterminal records as interrupted and publishes the new process fence.
Already-terminal records remain byte-for-byte unchanged. A crash, missing
acknowledgement or partial transaction never implies a native mutation did not
occur. Recovered facts cannot recreate refs, native contexts, provider state,
task predicates or executable approvals.

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

Accepting review produces `FreshAdmissionRequired`; rejecting it produces
`Rejected`. Both preserve unknown execution debt. Neither resumes the original
proposal, and neither means clean resource settlement. Stale/conflicting
decisions fail exact CAS; a new user decision cannot reopen a terminal record.
Any later approved execution needs newly admitted native authority and a fresh
trusted observation.

Deterministic tests cover the complete disposition-pair grammar, malformed
records, overflow, live-owner exclusion, restart classification, partial restart
and write rollback, lost commit acknowledgements, approval/reject/cancel races,
terminal immutability, retention pressure, callback loss/panic and bounded Store
mailbox admission. These are persistence proofs, not native/application workflow
qualification. Application admission and retained-controller reconciliation are
the next integration layer.
