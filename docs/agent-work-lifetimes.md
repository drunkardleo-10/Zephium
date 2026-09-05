# Sequential Work native lifetimes

Status: native factory prerequisite; application successor admission is not yet
wired or qualified. This ownership decision extends the Work foundation without
changing Browse or allowing concurrent native lifetimes.

## Context and decision

A Work run currently consumes the process's unique native port. Its terminal
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

Native emptiness alone is not task, policy or restart permission. The application
must separately require the original clean runtime join, provider/policy/audit
closure, exact immutable durable terminal acknowledgement, and delivery of
retained output/events before replacing its coordinator. That application
boundary must preserve the old handle as terminal, keep the exact engine/Store
identity and process fence, and reject uncertain or unfinished predecessors.
Fresh run input must still carry newly approved task/profile/manifest authority.

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
