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

The optional [application admission boundary](agent-work-persistence.md) now
owns durable admission and the complete runtime lifecycle. The composition root
prepares `PreparedAgentWork`, attaches the same Store's journal port to the
shell, and submits through `AgentWorkApplicationHandle`; native creation waits
for both durable admission acknowledgements. No UI or default-desktop agent
dependency is enabled by this seam.

For direct trusted compositions, construct `AgentWorkController::try_new` with the transport configuration,
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
- Action tasks advertise only locate and snapshot-verifiable act. A trusted
  `AgentWorkExtractionTask` instead advertises only initial-scope extraction;
  its [bounded result](agent-work-results.md) remains explicitly model-mapped.
  Immediate and
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

Task success is separate from resource cleanliness. The actor can now publish
`ClosedUnsuccessfully` after a fully accounted provider/task refusal or stop,
using those same original resource, metric, policy, audit and runtime proofs.
The exact typed cause remains visible and no extraction result is returned.
The application publishes distinct immutable `Failed` or `Cancelled` only
after the original clean lifecycle join and exact durable terminal ACK. Neither
fact grants another execution or reopens the sealed native port.

This path consumes only exact read-only and successful revocation callbacks;
foreign, duplicate, refused and lost receipts remain recovery debt. It shares
the existing cleanup deadline, including any shorter shutdown deadline, and
never retries an action. A retained native action/proposal/accounting owner,
audit refusal, or a previously started terminal-resource close cannot be
reclassified as clean. Failure during an already-started success close remains
recovery even when the page has physically closed.

Recovery retains the sealed session, policy, journal, native owners and retained
callbacks without credentials or objective data. It deliberately grants no
replay permission. The application may now explicitly reconcile the original
retained audit ledger and an exact uncertain durable write. Failure-path audit
events that have not been acknowledged remain debt, not claimed durability.
Persisted recovery facts classify interruption after restart; they do not
restore live executable state or fabricate a clean controller. Approval review
requires fresh admission and never executes an old proposal.

## Evidence and remaining product seams

The excluded public Luna qualifiers compose this actor with the actual
EngineHost context port, runtime mailbox and `SqliteStore` audit implementation.
The application qualifier additionally uses the [trusted macOS composition](agent-work-composition.md),
actual shell admission, journal and application shutdown. It requires trusted
task completion, durable success, focus isolation and clean native/store/worker
teardown. See the [M6 record](../eval/agentic-browsing/m6-production-qualification.md).
Only explicit public qualification can enable retained provider logs;
production/BYOK remains `store:false`. Local diagnostics contain only closed
states, correlations, counters and timings; no page/provider data or secrets.

Next: trusted product task/plan authoring and a user-facing Work command over
the opt-in Rust desktop admission port. No UI/IPC authority is added, and the
full Tauri window/bootstrap path is not live-qualified. Local/hosted model
transport adapters must share the same session semantics. Native macOS
suspend/resume, authenticated/public multi-site qualification, concurrent Browse
interaction, navigation and richer tool adapters remain open. A suspension
request currently revokes and closes: it is `Cancelled` only if all original
owners drain, otherwise recovery. It does not claim an unimplemented native
suspend operation succeeded or retain resumable execution.

The production extraction result is delivered once in memory after durable
terminal acknowledgement. Explicit durable-profile tasks can additionally
publish an atomic private [artifact](agent-work-artifacts.md); historical reads
never change a prior execution disposition. Content never enters the existing
audit/journal, and unsuccessful closed runs publish no artifact.
