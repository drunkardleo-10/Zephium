# Trusted local form preparation

`AgentWorkFormTask` is a production `AgentWorkTask` implementation for explicit
field/value postconditions. The trusted product can now supply bounded data
instead of writing a new Rust completion predicate for each form workflow.
It does not create a planner, interpreter, actor, native port or policy authority.
It uses the existing controller and durable application/composition path.

The supplied account scope is an initial product assertion, not a live sign-in
detector. Later samples retain its original identity/time. Longer work needs an
independently sourced [account adapter](agent-work-execution.md#stops-audit-and-terminal-truth);
this task does not renew authority by changing a timestamp.

## Trusted admission, not automatic effect classification

`try_new_local_preparation` is an explicit product attestation: changing these
fields on this exact context and origin has been independently classified as
local, non-submitting preparation. `Fill` and `Select` are **not** universally
local effects. Autosave, checkout, messaging and remotely effective fields need
their own trusted effect assessor; an origin allowlist or LocalWrite manifest
alone is insufficient. Page/model instructions cannot supply this contract.
There is deliberately no deserializer, UI/IPC endpoint, model-authored predicate
or automatic objective-to-contract conversion.

The caller supplies the exact owned context identity, native origin, trusted
account scope and ordered phases. Each phase has explicit requirements:

- `fill`: a textbox/searchbox's exact complete value, including explicit clear;
- `select`: a combobox's uniquely named descendant option, selected alone.

An optional exact accessible name narrows a field. Without a name, exactly one
field of that kind must exist in the complete initial root-frame observation.
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
target, option, action kind, local effect and exact fill text. Wrong values,
other fields, skipped phases and retired refs cannot pass assessment. Each new
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

The release-excluded public application qualifier now uses this production
contract for its query/language phases; it no longer implements a custom action
predicate. Its separate result consumer still independently checks the public
cited value. The [M6 record](../eval/agentic-browsing/m6-production-qualification.md)
contains exact full-application evidence and deterministic failure coverage.
Production and BYOK remain stateless; only the excluded public qualifier retains
provider logs.

This is a reusable contract for a bounded subset of work, not general task
authoring or arbitrary-site effect discovery. Goals, phase position and refs
are not restored after restart; the existing journal classifies interruptions
without replay. Higher-level trusted plan admission, reviewed site/effect
contracts, cross-document navigation, richer tools and parallel work remain
separate seams. No user UI, performance/battery or broad-site claim is added.
