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

For independently classified local form preparation, the production
[`AgentWorkFormTask`](agent-work-forms.md) supplies bounded trusted field/value
phases and exact fresh-target assessment without a bespoke Rust predicate.
It is not an automatic effect classifier for arbitrary forms.

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
and shutdown. The selected browser profile must already have its authoritative
content policy applied by the ordinary profile/policy owner before constructing
a Work page. Application admission waits for that existing readiness under the
original deadline; inventing a profile or installing a task-specific fallback
policy is not a repair. An uninstalled policy is a refusal, not permission to
bypass profile security. The [composition binding](agent-work-composition.md)
preserves the actor-selected session's profile and persistence class.

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
  A trusted task can explicitly combine these existing tools: its frozen schema
  and mode require fresh `ReadyForExtraction` before mapping and refuse further
  actions afterward. All phases share the same eight-call ceiling and original
  policy, native and result-publication owners.
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

The current Work capture admits its exact main frame only. Embedded frames are
retained as explicit `Unsupported(PolicyBlocked)` boundaries before observation
assembly, including in a requested subtree. They do not erase usable main-page
content or silently disappear, and they authorize no child capture, origin
inference, native call or action. Read/extraction projection marks a parent with
unobserved child boundaries `source_incomplete` even when the native main-frame
snapshot itself completed. The task must be satisfied by the actual cited
main-page evidence; embedded content remains outside this controller's scope.

Trusted extraction schemas can also narrow mapping evidence to a frozen closed
set of semantic source roles. This filters the already-authorized bounded
capture after privacy checks, not the native traversal or initial inspection.
It introduces no model tool option, new account/origin/frame authority, retry or
larger ceiling. `role_selection` distinguishes intentionally excluded readable
fields from `source_incomplete`; the exact selection is schema/read/citation
bound. The default remains all roles. See [results](agent-work-results.md).

Account evidence is sampled through the trusted task before each provider or
effect admission, including locate/read continuations, verified-action diffs
and extraction mapping. The actor owns that outer inspection loop; several
model calls cannot hide behind one startup sample. It checks sticky controls
before and after the bounded, synchronous account adapter. This adds no worker,
timer, native capture, model request or account-discovery mechanism.

The session freezes the admitted account and complete context/document join.
New samples must be fresh under the unchanged 30-second core policy limit,
non-regressing and non-future. An attestation identity cannot be rewritten or
replayed, and the fixed inventory is bounded by the model/effect ceilings.
An unchanged cached sample keeps its original age. Refresh over an original
provider/effect reservation refuses without replacing that owner. Refusal is
sticky and new identity requires fresh authorized admission, even when both
accounts appear in a manifest.

The built-in form/extraction predicates contain a constructor-supplied account
scope, not a live sign-in detector. They now preserve their first account sample
instead of minting newer timestamps. Without an independently sourced account
adapter they still fail closed when that sample expires. A trusted task may
delegate its predicate to these types and supply fresh account facts through
`attest_account`; neither page/model claims nor a timestamp update are evidence.
Native authenticated-account discovery and monitoring remain unimplemented.

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
never retries an action. A retained native action/accounting owner,
audit refusal, or a previously started terminal-resource close cannot be
reclassified as clean. Failure during an already-started success close remains
recovery even when the page has physically closed.

An exact never-dispatched policy `NeedsHuman` proposal is separately typed and
retained through original unsuccessful closure, then discarded without replay.
It keeps the original supervisor token for drain instead of yielding it away.
The application can then require [explicit durable review and fresh admission](agent-work-review.md).
Other proposal/admission failures remain conservative recovery owners.

Recovery retains the sealed session, policy, journal, native owners and retained
callbacks without credentials or objective data. It deliberately grants no
replay permission. The application may now explicitly reconcile the original
retained audit ledger and an exact uncertain durable write. Failure-path audit
events that have not been acknowledged remain debt, not claimed durability.
Persisted recovery facts classify interruption after restart; they do not
restore live executable state or fabricate a clean controller. Approval review
requires fresh admission and never executes an old proposal.

## Signed-in origin grants

Zephium never infers an account. A routine agent request may carry
`WorkAgentGrantV1.accounts`: each entry names one exact HTTPS origin, an
opaque account identity Rust minted when it drafted the approval
(`WorkAccountApprovalRequestV1` with `mode: origin`), and a page budget of at
most 12. The person's approval of "read pages on this origin as this account
for this request, up to N pages" is the attestation; nothing observes or
detects the account. A drafted grant is claimed once, by the next request of
its work: an undrafted or already used grant is refused as `ReviewRequired`,
and the grant ends with that request's execution.

Inside a granted origin the agent loop admits `read` directly. The page opens
in a Work-owned page sharing the profile's cookies, the same construction
`AccountRead` uses, counted against the budget and read with the same typed
records as a public page. Every other read stays anonymous, in the run's own
isolated storage; a signed-in page never joins the grouped anonymous pages and
runs as its own lifetime. The engine refuses a load that leaves the approved
document, so a redirect out of the origin ends the read as "The page left
<host>", and subresources from other sites carry no cookies. A signed-in page is
read-only: its page agent may only scroll, a path that names a change (sign
out, delete, send, pay and the like) is refused before it opens, and either
refusal reaches the model as `AccountWrite` with a notice to propose the
change as a field update. `AccountUpdate` remains the only way to change
anything on a signed-in page. Facts from a signed-in page never ride a search
query, its title is not recorded, provider retention never applies to it, and
diagnostics carry only the grant index, the path class, byte counts and
outcome classes. The projection marks each such page (`WorkStepFact.account`)
and the request's use of each origin (`WorkExecutionFact.accounts`).

With the person's consent for one request, `WorkContextSelectionV1.tabs`
lists the open tabs of the focused window as title, host and path (at most 60,
never content or query). The agent sees them as `tabs`; reading one is
anonymous unless its origin is granted.

## Evidence and remaining product seams

The excluded public Luna qualifiers compose this actor with the actual
EngineHost context port, runtime mailbox and `SqliteStore` audit implementation.
The application qualifier additionally uses the [trusted macOS composition](agent-work-composition.md),
actual shell admission, journal and application shutdown. It requires trusted
task completion, durable success, focus isolation and clean native/store/worker
teardown. See the [M6 record](../eval/agentic-browsing/m6-production-qualification.md).
Only an explicit synthetic qualification can enable retained provider logs;
production/BYOK remains `store:false`. Local diagnostics contain only closed
states, correlations, counters and timings; no page/provider data or secrets.

The first exact authenticated read-only workflow is separately recorded in the
[macOS Notion qualification](../eval/agentic-browsing/macos-authenticated-notion.md).
It proves one user-attested disposable-workspace task, same-document progressive
inspection, source-bound extraction, durable terminal publication, and clean
retained-resource reuse. It does not implement or qualify native account
discovery, writes, arbitrary Notion pages, Windows, or concurrent Browse.

Next: trusted product task/plan authoring and a user-facing Work command over
the opt-in Rust desktop admission port. No UI/IPC authority is added, and the
full Tauri window/bootstrap path is not live-qualified. Local/hosted model
transport adapters must share the same session semantics. Native macOS
suspend/resume, broader authenticated/public multi-site qualification,
concurrent Browse interaction, navigation and richer tool adapters remain
open. A suspension
request currently revokes and closes: it is `Cancelled` only if all original
owners drain, otherwise recovery. It does not claim an unimplemented native
suspend operation succeeded or retain resumable execution.

The production extraction result is delivered once in memory after durable
terminal acknowledgement. Explicit durable-profile tasks can additionally
publish an atomic private [artifact](agent-work-artifacts.md); historical reads
never change a prior execution disposition. Content never enters the existing
audit/journal, and unsuccessful closed runs publish no artifact.
