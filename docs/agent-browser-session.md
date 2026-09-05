# Bounded semantic browser session

`zephium-agent-controller::AgentBrowserSession` is available on the normal
`provider-transport` feature path. The production-path
[`AgentWorkController`](agent-work-execution.md) now drives this session on the
existing runtime worker, with the real browser-context, supervisor, audit and
lifecycle authorities. `TerraTextOnlyController` remains a legacy adapter;
the opt-in [macOS composition](agent-work-composition.md) now connects the actor
to real shell admission, native context ownership and durable terminal facts.

The session removes the diagnostic two-action control flow: one owned policy
and transport can process up to eight provider turns and eight native actions.
Each act turn contains exactly one action. Locate uses the existing bounded
Rust semantic matcher, and an action continuation requires the functional
core's independently verified, policy-accounted result and exact baseline diff.
There is no automatic action retry or successful continuation after a refusal.

Snapshot actions require at least 2,000 ms of total native dispatch, settlement
and adjacent-observation allowance, both in the provider schema and independently
before Rust policy/native dispatch. This is not a minimum wait: immediate/quiet
settlement still proceeds as soon as ready. The existing 30 s ceiling, run
deadline and exact postcondition remain unchanged. A controlled accepted-build
comparison exposed 1,000 ms budgets expiring during fresh native capture; no
dispatched deadline is extended and native throttling is not disabled. The
driver retains the exact typed verifier refusal as well as the original charged
failed-effect owner. See the M6 record for the comparison and qualification.

## Host contract

1. Construct the session with `try_new` before polling `start_initial`. This
   reserves no model budget and makes the asynchronous owner explicit.
2. `next_action` processes model-requested locate turns and binds one action
   against the complete native-current frame cohort. Its callback receives
   only typed content-free accounting and timing receipts.
3. Independently classify the prepared action's actual effect/destination,
   sample the current context registry, and call `authorize_action`.
4. Deliver the returned move-only request through
   `AgentRuntimeBrowser::execute_semantic_action` (or the exact native adapter
   in an excluded qualifier). Retain the session while the request is live.
5. After the declared bounded settlement condition, obtain a fresh semantic
   observation and call `settle_action` with the exact native terminal. This
   composes policy dispatch, native correlation, settlement, independent
   snapshot evidence, policy charging and accounted-result finalization.
   An asynchronous host instead uses `begin_action_settlement`, the core's
   exact `wake_action_settlement` schedule, and `verify_action_settlement`.
   Obtain only the adjacent fresh snapshot after settlement; polling with
   unrelated snapshots cannot substitute for the core's exact evidence join.
6. Evaluate the user's task-level postcondition from trusted observed state.
   Stop explicitly when it holds, or consume the verified transition with
   `continue_after_verified_action` and repeat. Model text cannot certify task
   success.
7. Call `try_finish` after native work is reconciled. Success proves only that
   this provider/session owner is drained. Refusal returns the sealed session,
   including pending accounting/action owners; it is not permission to retry.

Cancellation is sticky for provider admission. A host must also revoke native
context authority on human takeover, suspend, navigation or cancellation;
native callbacks must still be reconciled, never treated as cancelled-success.
Dropping an in-flight provider future leaves its attempt inside the session so
the host can abort and settle it. Credentials are zeroizing, memory-only and
released on close. No page/provider content is emitted by this driver.

The host must durably audit the content-free model/effect receipts and join its
own supervisor, resource and native shutdown proofs. The sealed terminal keeps
all charged model receipts (including failed terminals) and the policy owner;
it deliberately does not manufacture an `AgentRunMetricClosure` or a durable
audit acknowledgement.

Only the owning Work actor has a private unsuccessful resource-drain path. It
preserves a failed/cancelled task outcome while joining the original provider,
action, policy and audit owners with the native/runtime terminal proof. It is
not an alternative public successful `try_finish`, and cannot discard retained
action or callback debt. See [Work execution](agent-work-execution.md).

## Current limits

An explicit trusted extraction task can instead select the initial-scope,
extract-only path. `extract` consumes the same session's provider continuation,
charges a purpose-bound mapping call and returns validated, cited model-mapped
data. It does not turn model text into a native effect or completion proof.
The actor and application gate the one-shot result on their original closure
and durable terminal ACK. See [Work results](agent-work-results.md) for bounds
and trust, and [Work artifacts](agent-work-artifacts.md) for explicit private
durable-profile publication.

- Snapshot-verifiable actions only. Navigation, dialog and scroll evidence
  require distinct host adapters and are explicitly refused.
- Immediate and mutation-quiet waits only. State/navigation/dialog/scroll waits
  require explicit host adapters; the bounded driver excludes their schemas
  and independently refuses them before native dispatch.
- An incomplete settlement remains a typed retained recovery state. The Work
  actor owns native callback/timer settlement and adjacent fresh snapshots;
  explicit post-failure reconciliation/resumption remains an application seam.
- Model selection is centralized in the trusted catalog: the supplied transport
  adapters are currently GPT-5.6 Terra and Luna. Both use the same semantic
  loop. Local/other hosted adapters and the Work UI wiring remain outstanding.
- Only locate and act schemas are advertised, in every OpenAI/Anthropic
  request serializer. Capability restriction is part of continuation config
  equality. Advertising support does not replace Rust tool/action validation.
- Production and BYOK requests use `store:false`. Only the release-excluded
  explicit public qualifier can opt into retained provider logs.

The excluded native workflow harness may exercise this driver, but a successful
single public workflow would not by itself establish production qualification.

## macOS scheduling ownership

Ready Work-owned views use WebKit's `Throttle` inactive scheduling policy,
attested from the live configuration at construction. Hidden views remain
runnable with background CPU limits; Browse's scheduling policy is unchanged.
WebKit's default `Suspend` can stop an inactive view's tasks despite Rust still
owning an authorized in-flight semantic request. The policy meanings and
default are defined by [Apple's WKPreferences documentation](https://developer.apple.com/documentation/webkit/wkpreferences/inactiveschedulingpolicy-swift.property).

There is no per-request full-speed override or shared mutable restore guard.
Refusals, callback timeouts and teardown retain the ordinary owned-view close
path. Explicit native macOS suspend/resume is not yet wired into the host (the
existing adapter is Windows-only); this change does not claim suspended-page
or idle battery qualification. A future macOS suspend adapter must revoke
automation authority before parking the page and re-attest on resume.
