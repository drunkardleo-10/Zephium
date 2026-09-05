# Closed refusal, review and fresh Work admission

This opt-in production seam handles an exact policy `NeedsHuman` refusal before
an action permit or native dispatch. It does not approve a model-authored
action, resume a suspended session, or replay a mutation. Trusted task/effect
contracts and explicit new admission remain required.

The previous generic refused-proposal owner prevented terminal drain even when
no action had been dispatched. Yielding the supervisor into a human wait also
invalidated its original execution token. The actor now records the exact
content-free refusal while retaining that token for cleanup. Progress counts
the refusal without inventing an in-run human-wait duration: product review
happens after this run has closed.

## Original owners, not inferred emptiness

Only the policy's exact `NeedsHuman` branch, before permit/dispatch and after
successful supervisor/audit/progress recording, marks a proposal as unissued.
Every other authorization/admission error retains the existing conservative
owner. The actor's private unsuccessful drain may carry this typed unissued
owner through shutdown; public successful session finish still refuses it.

The original prepared action, batch and provider continuation survive every
native/audit/runtime refusal. They are discarded only after original native
and provider shutdown proofs, metric/policy/audit closure and runtime terminal
commit. Credentials and objective are released on session sealing. No native,
provider, settlement or accounting debt can be cleared by this classification.
A takeover/cancellation winning during drain produces its original cancelled
closure and no clean human-review marker.

After the exact clean worker lifecycle join, the application may persist
`Running → NeedsApproval` with zero debt using the original failed
`PolicyDenied` closure, native proof and exact manifest/run-bound human refusal.
The generic `NeedsApproval` transition still preserves unknown debt. Earlier
verified effects, if any, remain charged and audited; “unissued” describes only
the refused proposal, not a claim that the whole run performed no effects.

## Review is a durable decision, never execution permission

Accept writes immutable `FreshAdmissionRequired`; reject writes `Rejected`.
Stopping an already-closed review writes `FailedClosed`. All preserve the exact
prior debt. The first retained terminal CAS wins; a late stop cannot overwrite
its decision. If stop arrives while the nonterminal `NeedsApproval` write is
pending, its exact ACK is reconciled before the fail-closed review write.

Only after the exact decision ACK does the current projection become `Reviewed`
for a zero-debt review. Missing ACKs retain the original CAS for the existing
bounded explicit reconciliation path. Stale/replayed/foreign decisions cannot
reopen a record. Accepting a review with unresolved debt still leaves Recovery.

`attach_successor_work` accepts a Reviewed predecessor only when the application
also retains its original clean lifecycle, native proof and failed-review
outcome, with all event/output/persistence lanes drained. A decoded historical
zero-debt record is not that proof. The same Engine/Store allocation, process
fence and native lifetime factory remain required. Each old port stays sealed;
the new run gets a distinct native context, fresh observation and independently
supplied trusted task/manifest/provider input. Rejecting one proposal does not
prevent a separately authorized different task, and never grants the rejected
proposal authority.

## Persistence and limits

The fixed content-free 96-byte version-one envelope now accepts zero-debt review
classifications. Older readers reject those previously invalid combinations
rather than guessing. No schema migration, objective, proposal, page/provider
body, credential, URL or executable continuation is stored. Reviewed terminals
remain immutable across restart. An unreviewed `NeedsApproval`, including one
with zero recorded debt, becomes `Interrupted` with unknown debt after process
restart: the old process's closure owners cannot be reconstructed.

There is no new worker, queue, timer, transport, default feature or Browse
scheduling change. This is explicit serial execution, not automatic retry or
concurrent Work. It does not provide UI approval authoring, executable recovery
after restart, or a reviewed site/effect classifier. The excluded macOS Luna
qualifier exercises refusal → explicit review → fresh authorized form run
through the actual Shell/SQLite/composition/native path; see
[M6](../eval/agentic-browsing/m6-production-qualification.md) for narrow evidence.
