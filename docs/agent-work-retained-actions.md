# Retained semantic action ownership

The retained resource core now owns pending semantic actions alongside pending
reads and navigations. The app lease adapter and common controller now connect
this ownership to the existing action authorization, execution accounting, fresh
observation and verification loop. Retained discovery remains read-only: its
task assessor refuses effects, and discovery admission refuses action-capable
tasks. A separate trusted task must supply the actual effect contract.

## Authority and lifecycle

`WorkBrowserResources::prepare_action` accepts an existing
`SemanticActionNativeRequest`. That request is created by the semantic action
coordinator only after the independent policy owner approves and dispatches the
exact effect. A retained resource lease, model proposal, accessible label or
action kind cannot substitute for that authority. In particular, clicking a
button or changing a field on an arbitrary site is not assumed to be read-only
or local: page event handlers can commit remote effects.

Resource admission checks the exact lease, frame/document generation, latest
observed semantic invocation and snapshot, resource health, and both native and
lease deadlines. An outstanding observation, navigation or action excludes a
new action. Admission immediately retires previous observation authority. An
action's native deadline must fit within the original lease deadline; this
protocol never extends either deadline or silently changes the native recipe.

The resulting move-only request transfers the existing closed recipe to the
native executor and retains an exact completion owner. The owner accepts only
the matching native terminal, using the full private semantic correlation.
Mismatch returns both original operands for recovery. Synchronous dispatch
refusal returns the original request and recipe for independent policy refusal;
it manufactures no callback or successful action result.

`settle_action` clears only that original resource obligation. It returns the
same native terminal to the policy/verification owner even after lease expiry,
revocation, clock failure, quarantine or resource destruction. Its `is_current`
flag describes lease/document health, not whether an effect succeeded. Native
outcome admission, a fresh observation, effect verification, and policy/audit
settlement remain separate requirements. The previous references remain retired
after both callback completion and synchronous dispatch refusal.

Pending action debt prevents successful lease revocation, capacity reuse,
resource reaping and global native shutdown. Native destruction does not erase
a callback that can still arrive. Losing an owner leaves explicit debt; no
timer, wake, cancellation request or zero count can manufacture its receipt.
Each resource retains at most one small action correlation and no second action
payload, worker, timer or unbounded queue.

## Native and controller integration contract

See [native retained action integration](agent-work-native-actions.md) for the
engine guard, native execution and platform-specific evidence.

Native callback arrival is not callback return. Each action carries a separate
move-only delivery ticket and native completion, using the existing bounded
delivery rendezvous behind action-specific types. The app registers one listener
before dispatch and retains the terminal until the exact native callback has
returned and its action/task guard permits a new observation. Native publishes
that physical fact and wakes outside locks. A missing publisher, callback panic,
or lost notification remains unproven. Revocation does not erase the original
terminal or prevent publication of a genuine physical return during cleanup.

This barrier closes an in-process race: an awakened worker could otherwise see
the terminal and request fresh state while native still holds the action guard.
Neither time delays nor automatic observation retries substitute for the barrier.

`AgentBrowserPort::work_resource_act` defaults to lossless `Unsupported`.
Enabling an adapter requires all of the following together:

- The engine guard admits the original lease and observed checkpoint, and owns
  action/callback debt through the actual callback return. The host dispatches
  on the existing retained WKWebView or WebView2, with exact resource/document
  checks immediately before applying the fixed recipe.
- Native recipe execution keeps the existing semantic target revalidation,
  input restrictions and deadline. It must not route retained requests through
  the legacy owned-context registry or permit unapproved document transitions.
- Cancellation or human takeover closes new action admission before native
  dispatch. Already dispatched work retains its terminal and physical callback
  owners until drained; an uncertain effect is not replayed automatically.
- The app lease adapter retains the original request/completion/delivery owners.
  The Work controller rejoins the original policy reservation, captures fresh
  post-action state, verifies the declared result, and settles audit ownership
  before another model decision or publication.
- An action-capable task supplies a trusted effect assessment and approved scope.
  Public discovery never gains write authority merely because a tool is exposed.

The existing form task is an example of an explicitly trusted local-preparation
contract; it is not a safe generic assessor for arbitrary production websites
with autosave, submission or account actions. The product must supply the actual
approval contract for those effects.

`AgentWorkFormTask::with_extraction` composes that exact trusted goal contract
with a source-bound result. The controller exposes actions only while those
goals are unfinished, requires independent post-action observation and effect
verification, and then switches to extraction. A model-proposed field value,
accessible label, or extraction response cannot create or change a goal.

Cancellation/human takeover seals admission before waiting for outstanding
native work. The app still drains the original action and physical-return
owners. If stop prevents post-action verification, the controller retains the
original policy reservation and terminal in recovery; it does not claim a
verified effect or retry the action. Resource drain and policy closure remain
independent. If the controller disappears, the app retains an orphaned native
terminal rather than throwing effect evidence away during physical cleanup.

## Evidence

Deterministic core tests exercise exact native/coordinator receipt preservation,
exclusive operation admission, fresh observation after actions, stale semantic
checkpoints, foreign resources, deadline boundaries, synchronous refusal,
substituted terminals, revocation, expiry, quarantine, clock regression, and
destruction with a missing action callback. They isolate resource ownership;
they do not claim native retained action execution or a successful real-site
workflow. App loopback tests now additionally exercise the real common
controller/provider/policy path: explicit local-write approval, one exact fill,
fresh post-action verification, source-bound extraction, refusal without plan
authority or with a different model-proposed value, synchronous native refusal,
and cancellation/human takeover with an already-applied late terminal. A
deterministic poll inside the native callback proves terminal arrival cannot
release action debt before the physical-return publication. These tests use
isolated native and model fixtures; a bundled native witness and real-site
qualification remain separate evidence requirements.
