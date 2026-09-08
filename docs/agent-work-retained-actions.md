# Retained semantic action ownership

The retained resource core now owns pending semantic actions alongside pending
reads and navigations. This is the resource and native-port contract for the next
action-capable Work integration. The shipping retained discovery task remains
read-only: its effect assessor, controller admission, app lease adapter and
native executor do not yet admit retained actions.

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

## Evidence

Deterministic core tests exercise exact native/coordinator receipt preservation,
exclusive operation admission, fresh observation after actions, stale semantic
checkpoints, foreign resources, deadline boundaries, synchronous refusal,
substituted terminals, revocation, expiry, quarantine, clock regression, and
destruction with a missing action callback. They isolate resource ownership;
they do not claim native retained action execution or a successful real-site
workflow. Those require the integration above and a bundled native witness.
