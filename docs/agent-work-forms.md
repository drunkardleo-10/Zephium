# Trusted bounded form transitions

`AgentWorkFormTask` is a production `AgentWorkTask` implementation for explicit
field/value postconditions. The trusted product can now supply bounded data
instead of writing a new Rust completion predicate for each form workflow.
It does not create a planner, interpreter, actor, native port or policy authority.
It uses the existing controller and durable application/composition path.

The supplied account scope is an initial product assertion, not a live sign-in
detector. Later samples retain its original identity/time. Longer work needs an
independently sourced [account adapter](agent-work-execution.md#stops-audit-and-terminal-truth);
this task does not renew authority by changing a timestamp.
Product Work on the person's sites runs as [page tasks](agent-work-execution.md#working-as-you),
not through this task.

## Trusted admission, not automatic effect classification

`try_new_local_preparation` is an explicit product attestation: changing these
fields on this exact context and origin has been independently classified as
local, non-submitting preparation. `try_new_external_update` is a separate,
equally explicit product attestation for exact remotely effective field updates.
It does not infer remote-write safety from a field role, action kind, objective,
page, model, or origin allowlist. The approved plan and product-owned task must
already know the exact target state transition and classify it as
`ExternalWrite`. Page/model instructions cannot supply either contract.

`Fill` and `Select` are **not** universally local or remotely safe effects.
Checkout, messaging, destructive controls, and open-ended autosave fields need
their own trusted contracts and approval semantics. An effect-bearing manifest
alone is insufficient.
There is deliberately no deserializer, UI/IPC endpoint, model-authored predicate
or automatic objective-to-contract conversion.

The caller supplies the exact owned context identity, native origin, trusted
account scope and ordered phases. Each phase has explicit requirements:

- `fill`: a textbox/searchbox's exact complete value, including explicit clear;
- `fill_transition`: an exact complete prior value and exact destination value;
- `select`: a combobox's uniquely named descendant option, selected alone.

An optional exact accessible name narrows a field. Without a name, exactly one
field of that kind must exist in the complete initial root-frame observation.
For a transition, the prior/destination values also narrow the candidate field.
Any third current value refuses the task; an already-observed destination is
satisfied without manufacturing an action. Identical prior/destination values
are not transitions and are rejected at construction.

Ambiguous or missing fields/options, unsupported operations, secret/disabled
controls, truncated snapshots, partial expansion scopes, changed origins or
context generations fail closed. These are semantic names, not DOM selectors.
No ordinary tabs, child-frame targets, listbox/multiselect, clicks, key presses,
submission or navigation are admitted by this task implementation.

## Fresh target and completion binding

All requirements in a phase must hold together in independently captured state.
The model may fulfill them in either order. A later phase cannot authorize a
write until the earlier phase is observed satisfied. Already-satisfied phases
require no synthetic action; phase completion is not proof that an action ran.
Repeat unchanged fields in the next phase when they must remain satisfied.

Only unsatisfied current goals retain opaque target/option bindings. Assessment
checks the exact observation ID/generation, frame/context, snapshot generation,
target, option, action kind, constructor-selected effect and exact fill text.
Wrong values, effect classes, other fields, skipped phases and retired refs
cannot pass assessment. Each new
observation clears old bindings before validation; any evidence refusal latches
the task closed. Completed tasks cannot be reused. Native revalidation,
independent effect verification, policy admission/accounting, cancellation,
audit and durable terminal closure remain mandatory and unchanged.

Goals contain private trusted content but no diagnostic/serialization surface.
The task retains at most eight total field goals over at most eight phases,
at most eight current bindings and one content-free baseline, never a copied
page snapshot. Values are bounded to the exact 1,024-byte observable preview;
field/option names to 512 bytes. Existing eight-turn/effect/run ceilings still
apply, so a maximal contract is not a promise that a model can finish within its
budget. There is no additional worker, queue, timer, persistence, transport or
default Browse dependency/resource graph change.

## Qualification and limitations

The release-excluded public application qualifier uses the local contract for
its query/language phases. The authenticated Notion write witness uses two exact
external transitions for temporary title mutation and restoration. Both keep a
separate result consumer that independently checks the cited current field
value; neither turns a model statement into task completion. The
[M6 record](../eval/agentic-browsing/m6-production-qualification.md) contains the
local full-application evidence. Authenticated remote-write evidence is recorded
separately after its real macOS run. Production and BYOK remain stateless; only
excluded qualifiers retain provider logs.

The controller has an optional trusted-task initial-readiness gate (default:
ready), before any task progress or model call. An exact target that is present
but not yet usable may consume at most seven fresh startup observations with
asynchronous 250/500/1000/2000/2000/2000 ms backoff, within a ten-second local
window and the unchanged run deadline. Delay is only scheduling: fresh task
evidence must prove readiness. Account/control checks remain live; changed
context, document/frame, stale generations, ambiguous identity and exhaustion
fail closed. One-shot retained adapters cannot opt into repeated reads. This
gate is not used after actions and cannot retry a possibly observed mutation.

The Notion witness waits only while exactly one public original-title textbox
exists without native Fill capability. It rejects changed values, duplicates,
visible nested editor controls and foreign context immediately. The projection
does not expose every reason Fill is absent, so a persistently readonly or
unsupported host can exhaust the read-only wait; it never acquires write
authority by waiting. Fresh native Fill support is required before the first
checkpoint and model turn. This does not claim Notion hydration or autosave
has been qualified until the next real run is measured.

This is a reusable contract for a bounded subset of work, not general task
authoring or arbitrary-site effect discovery. Goals, phase position and refs
are not restored after restart; the existing journal classifies interruptions
without replay. Higher-level trusted plan admission, reviewed site/effect
contracts, cross-document navigation, richer tools and parallel work remain
separate seams. No user UI, performance/battery or broad-site claim is added.
