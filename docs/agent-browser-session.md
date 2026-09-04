# Bounded semantic browser session

`zephium-agent-controller::AgentBrowserSession` is available on the normal
`provider-transport` feature path. It is a reusable locate/act driver, not yet
the Work application's supervisor/lifecycle actor. The existing
`TerraTextOnlyController` remains the shipping runtime integration.

The session removes the diagnostic two-action control flow: one owned policy
and transport can process up to eight provider turns and eight native actions.
Each act turn contains exactly one action. Locate uses the existing bounded
Rust semantic matcher, and an action continuation requires the functional
core's independently verified, policy-accounted result and exact baseline diff.
There is no automatic action retry or successful continuation after a refusal.

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

## Current limits

- Snapshot-verifiable actions only. Navigation, dialog and scroll evidence
  require distinct host adapters and are explicitly refused.
- An incomplete settlement remains a typed retained recovery state. A general
  native-event/settlement polling actor and fresh-full-snapshot continuation
  are not implemented by this slice.
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
