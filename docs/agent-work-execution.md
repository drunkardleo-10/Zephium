# Work execution foundation

`AgentWorkController` is a production-feature implementation of the existing
`AgentRuntimeController`, not a diagnostic controller or a second automation
stack. It owns `AgentBrowserSession` and drives the real `AgentRuntimeBrowser`
port. This is an executable foundation, not yet a user-facing Work product or
a production qualification claim.

## Application boundary

The trusted application admits an `AgentWorkRunInput`: one approved manifest
root, exact owned-context/profile/initial-target assignment, task objective,
catalog model, stable identifiers, clock and absolute deadline. An
`AgentWorkTask` supplies a trusted task-level predicate, independent effect
classification and exact account attestation. Neither a model response nor
hostile page text may implement these authorities. Model text and action counts
cannot terminate a task successfully.

Construct `AgentWorkController::try_new` with the transport configuration,
move-only credential and existing `AgentAuditPort`. Move it into
`PendingAgentRuntime::spawn_suspended_with_controller`, bind the actual
`EngineHost::take_agent_browser_port`, and retain the existing runtime handle,
completion handle and lifecycle owner. Only explicit `start_run` begins native
or provider work. `AgentWorkHandle` exposes bounded content-free events and the
single move-only terminal/recovery outcome; it exposes no page/native internals.

The caller must keep pumping the engine's native dispatcher during execution
and shutdown. It must install the selected profile's authoritative content
policy before constructing a Work page. An uninstalled policy is a refusal,
not permission to bypass profile security.

## Ownership and bounds

The actor uses the original supervisor, policy, audit ledger and four metric
reducers. Its original context registry, profile leases, cookie-transfer and
screenshot coordinators join the session's original native execution and
settlement coordinators for shutdown. No replacement empty resource cohort
may manufacture a clean proof.

- One run, one root and one independently managed extension-free owned page.
- At most eight model turns and eight one-action turns; locate is bounded by
  the existing semantic matcher. No action or provider request is blindly retried.
- Only locate and snapshot-verifiable act tools are advertised. Immediate and
  mutation-quiet settlement use the core's bounded wake schedule before one
  adjacent fresh observation. Navigation/dialog/scroll adapters are absent.
- The absolute deadline is bounded by the approved root expiry and ten minutes.
  Explicit read-only `NotReady` receipts permit at most 64 readiness checks at
  50 ms intervals, with fresh invocation IDs but unchanged snapshot generation.
  Other failures do not trigger observation, action or model replay.
- Product progress has a preallocated 64-event FIFO with stable run/sequence
  correlation. Overflow is sticky backpressure; no overwrite or extra worker.
  The move-only terminal slot is separate, so progress pressure cannot discard
  an already-proven completion.
- Deferred native callbacks are bounded by the existing runtime mailbox's
  maximum terminal plus signal capacity. Overflow remains unclean recovery.

The existing single runtime worker now enables its I/O reactor as well as its
timer. Construction of an unused controller starts no worker, timer, request or
native page. The runtime still has no provider HTTP dependency. The default
desktop graph and Browse scheduling are unchanged. This is structural evidence,
not measured battery, RAM or CPU qualification.

## Stops, audit and terminal truth

Cancellation, human takeover, policy revocation and suspension requests use the
existing sealed control lane with a first-wins typed reason. Shutdown remains
the stronger lifecycle event. Renderer/navigation loss, mailbox faults,
deadlines and provider/native refusals fail closed. Rust invalidates old joins
and observations before native revocation. Already-dispatched callbacks remain
owned even when a provider future is aborted or native automation is revoked.

Recovery schedules at most one separately tracked lifecycle close after
revocation; it does not wait for a lost observation or cancellation callback to
exhaust cleanup time. Exact close acknowledgement may release the physical
context/profile owner without erasing any other callback/accounting debt.
Callback loss, close refusal and timeout retain that debt in `AgentWorkRecovery`.
Default recovery draining is bounded to one second, or the existing lifecycle
shutdown deadline. Closing a page is not evidence that a native action succeeded.

Success requires the trusted task predicate, independently verified and charged
effects, exact provider accounting/drain, durable content-free audit delivery,
the original native-resource shutdown proof, metric/policy closure, and an exact
runtime terminal claim with no mailbox debt. Runtime lifecycle Clean additionally
requires normal controller return and successful worker join. A business-only
commit, nonzero native audit, missing provider proof or lost audit callback can
never establish Clean.

Recovery retains the sealed session, policy, journal, native owners and retained
callbacks without credentials or objective data. It deliberately grants no
replay permission. This pass does not expose a recovery reconciliation/resume
command or persist live executable state across process exit. Failure-path
audit events that have not been acknowledged remain in the retained ledger;
they are not described as durable. A later application recovery adapter must
settle that exact debt, not launch a new controller over a fabricated clean run.

## Evidence and remaining product seams

The excluded public Luna qualifier now composes this actor with the actual
EngineHost context port, runtime mailbox and `SqliteStore` audit implementation.
It requires trusted task completion, durable closure, focus isolation and clean
native/store teardown. See the [M6 record](../eval/agentic-browsing/m6-production-qualification.md).
Only explicit public qualification can enable retained provider logs;
production/BYOK remains `store:false`. Local diagnostics contain only closed
states, correlations, counters and timings; no page/provider data or secrets.

Next: authoritative Work application admission/state persistence and explicit
approval/recovery reconciliation over this retained owner. The current desktop
does not yet start this actor from a user Work command. Local/hosted model
transport adapters must share the same session semantics. Native macOS
suspend/resume, authenticated/public multi-site qualification, concurrent Browse
interaction, navigation and richer tool adapters remain open. A suspension
request currently revokes and closes into recovery; it does not falsely claim
that an unimplemented native suspend operation succeeded.
