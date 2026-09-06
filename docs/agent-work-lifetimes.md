# Sequential Work native lifetimes

Status: shipping opt-in native/application/composition boundary, exercised by
the real macOS Shell/SQLite/Luna path. This ownership decision extends the Work
foundation without changing Browse or allowing concurrent native lifetimes.

## Context and decision

A Work run consumes a unique native port. Its terminal
proof requires that port to be permanently sealed, so constructing a second
controller cannot legitimately reuse it. Ordinary tabs, a replacement Store,
resetting a seal or starting another hidden engine are not successor authority.

The engine now offers a process-unique `AgentBrowserLifetimeFactory`, mutually
exclusive with the legacy exact-once `take_agent_browser_port`. Each `begin`
issues a distinct port with its own immutable sink and admission gate. No port
ever unseals. Taking the factory creates no worker, timer, native page or queue;
its fixed state exists only after explicit opt-in acquisition.

A successor requires the preceding port's exact native shutdown task to attest
all nine resource counts as zero while holding its sole remaining native task
permit. Afterwards all task/capture permits must be absent. Retirement and
old read-only audit admission share the same lock: a retired port cannot audit
a successor's resources. Native invariant failure is sticky across the entire
factory lineage, and whole-engine shutdown permanently seals the factory and
its current port. The factory retains one current admission owner, not a growing
history, and permits at most 1,024 lifetimes per process.
An intervening refused, lost, mismatched or nonempty ordinary audit cannot use
the earlier shutdown receipt to admit a successor. Invalidation occurs before
that audit releases its permit; retrying an ordinary audit cannot mint a receipt.

Native emptiness alone is not task, policy or restart permission. The application
separately requires the original clean runtime join, provider/policy/audit
closure, exact immutable durable terminal acknowledgement, and delivery of
retained output/events before replacing its coordinator. That application
boundary preserves the old handle as terminal, keeps the exact engine/Store
identity and process fence, and reject uncertain or unfinished predecessors.
Fresh run input must still carry newly approved task/profile/manifest authority.

`CallbackHandle::attach_successor_work` joins the exact predecessor projection
and immutable record to the Shell's retained original owners. Ordinary second
attachment stays refused. `Succeeded`, `Failed` or `Cancelled` with zero
debt and matching original outcome can qualify. A [proof-closed review](agent-work-review.md)
can additionally qualify only after its exact decision ACK and retained original
failed-review/native/lifecycle proof join; review never resumes an old action.
An active runtime, recovery
audit, uncertain durable write, staged input, undelivered event or unconsumed
result blocks replacement. The new coordinator reloads the same fenced Store;
it never restores a task or releases/reacquires the process fence. Old handles
remain terminal and retain their own immutable record, not quadratic copies of
later history. Their controls cannot target a new projection. No automatic run
queue, retry, background continuation or parallel execution is introduced.

The same dormant `MacosWorkComposition` lazily takes the process factory once.
Each prepared run still has a deferred `FnOnce`; the application invokes it only
after exact durable Admitted/Running acknowledgements. The opt-in desktop Rust
port `admit_successor_trusted_work` requires the exact prior handle and fresh
trusted input. Invalid preparation leaves the original composition intact;
mailbox refusal preserves the original prepared owner. Neither port is UI/IPC.

See [M6](../eval/agentic-browsing/m6-production-qualification.md) for the narrow
public native evidence. Deterministic coverage additionally checks successful
and failed predecessors, pending terminal ACK/result/event/native owners, stale
handles, wrong engine identity, lost callbacks and audit/retirement/shutdown races.

## Alternatives and consequences

Reopening the old gate would invalidate its terminal proof and allow stale
handles to execute. Treating a global shutdown proof as a per-run lease without
sealing would likewise weaken the existing runtime contract. Distinct sealed
lifetimes preserve those proofs and avoid a parallel orchestration stack.

This serial design deliberately does not implement concurrent Work runs or
per-context shutdown proofs. A refused, lost or nonzero native shutdown receipt
cannot be replaced by guessed emptiness. A future concurrent design must define
scoped native resource ownership and application proof composition explicitly;
it cannot remove these barriers or reinterpret old proofs.

Review/revisit triggers: physical callback isolation fails qualification,
concurrent execution becomes a concrete product requirement, or a platform
cannot prove complete native cohort retirement without process exit. No new
battery/resource qualification or default-desktop enablement is implied.

The subsequent [persistent Work-resource boundary](agent-work-resources.md)
separates durable page ownership from run-bound execution leases. Its first
macOS native slice supports an explicit frozen document and bounded initial
reads. One actual-application two-lease retention witness is qualified under the
separately qualified, release-excluded foreground holder, with exact resource
destruction and application closure. It does not reinterpret these accepted
serial terminal proofs: retained Work pages and their delivery owners now
explicitly block the same all-zero/successor gates. Product rendering authority
and general task execution remain separate integration work.
