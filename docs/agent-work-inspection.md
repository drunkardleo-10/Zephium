# Nonterminal baseline inspection

Trusted `AgentWorkTask` implementations may opt into `allows_baseline_read`.
`AgentWorkFormTask` and `AgentWorkExtractionTask` expose the corresponding
`with_baseline_read` builder. The actor freezes that capability at admission,
rechecks the task contract, and binds it into the same immutable provider config
as action/extraction capabilities. Existing tasks and default Browse do not
enable it. There is no new worker, native port, queue, persistent content, or
default dependency.

The model may request `read` with `scope: initial` before choosing its next
tool. The shipping `AgentBrowserSession::continue_after_read` projects bounded
public detail from the exact existing acknowledged observation. This can expose
collapsed option labels omitted by the compact initial model projection. Read
is useful for inspecting an inventory; targeted `locate` remains cheaper when
the desired semantic label is already known. Read is not a refresh, subtree
expansion, action verification, task-completion signal, or approval amendment.

## Exact ownership and disclosure

The original settled tool turn supplies the move-only continuation. Core binds
its requested scope as well as the exact observation fingerprint, config,
lineage, read guard and payload. A substituted scope fails before provider
admission. The actor supplies the original capture timestamp, never the time
of the later read. Secret values are mechanically withheld; sensitivity,
source incompleteness and item/byte omissions remain explicit.

The standard projection admits at most 128 fragments and 32 KiB content; the
shipping encoding additionally caps its entire semantic payload at 16 KiB.
An oversized encoded projection refuses rather than truncating a serialized
result or silently broadening a budget. Its conservative UTF-8 preflight is not
an exact token count: the existing authenticated full-request count and policy
reservation still precede generation. Exact-token budgets refuse conservative
admission. Model-call accounting, read-taint admission and audit delivery use
the existing authorities, including on failure.

A committed read receipt acknowledges only those read bytes. The continuation
retains the original observation acknowledgement separately; it cannot mint a
new observation or action-reference capability. Reads share the existing
eight-turn, 256 KiB transcript ceiling and run deadline. No repeated-read retry
or separate read allowance is introduced. After a verified action, a subsequent
read is bound to the freshly delivered action-diff baseline, not the old one.

The same actor provider pump handles read count/generation and cancellation,
takeover, revocation and shutdown. A stopped provider attempt remains owned until
settled. Already-dispatched actions, missing native callbacks and undelivered
audit records retain their existing recovery owners; neither a read receipt nor
its model response clears those debts. Product events reuse typed `Read`, model
accounting and existing terminal/recovery projections. Page/provider contents
are never local diagnostics.

## Scope and evidence

This is a nonterminal **initial-baseline** read only. Native subtree reads remain
terminal schema-bound extraction; no navigation, generic snapshot, arbitrary
JS/DOM, visual read, new-origin authority, or richer native adapter is implied.
The direct `next_action` session convenience method remains locate/act; the
shipping Work actor supplies the additional trusted capture-time/capability
contract. Production/BYOK remains stateless, while the release-excluded public
qualifier explicitly opts into retained public logging.

Deterministic tests cover read→read→verified action, read-only extraction,
verified action→read→cited result, capability changes, unsupported scope,
disabled reads, count/generation refusal, cancel/takeover/suspend, the exact
turn ceiling, native callback loss and audit debt. They assert no extra native
captures and no action on read-only/refused paths. Core tests cover exact scope,
config, baseline, token quality and payload binding for both provider codecs.
Real application measurements are kept out of this document.

## Native keyword discovery for progressive observations

Discovery tasks that already admit progressive native snapshots can request
`snapshot({scope: {kind: "text_search", target: "@a1", query: "width depth availability"}})`.
The target must be a document, landmark, group or dialog reference from the
exact acknowledged observation. Its current native source bounds the walk;
the query cannot widen that scope, cross a frame, navigate, click, or access a
hidden or editable subtree. `locate` continues to search only the current
decoded observation. Native text search is how a model can discover rendered
evidence omitted by that compact observation.

Queries are literal Unicode words/numbers, matched case-insensitively by any
word, with a 256 UTF-8-byte ceiling. Distinct matched words rank candidate
passages; equal scores preserve traversal order. This is deterministic lexical
retrieval, not embedding search or a claim of semantic understanding. Concise
discriminating page words, numbers and units are appropriate. There is no
stemming or synonym expansion: `availability` does not match `Available now`,
and `price` does not match a currency amount by itself. Symbol-only queries
match the entire trimmed query as a literal substring, so `$`, `€` or `%` can
find unlabeled amounts. A symbol-only query has one match score; punctuation
in a query containing words/numbers remains a separator, not another OR term.
For example, `$[]` searches for those three consecutive characters, without
regex interpretation. Blank queries, controls, unsafe formatting characters
and secret-like values remain rejected. The provider tool
description states this contract so the model can choose actual page wording
after a lexical miss. Query strings never become selectors,
regular expressions, JavaScript, attributes or page instructions.

The fixed runtime retains at most 16 contiguous source passages, each at most
4 KiB, and emits at most 8 KiB of text plus the region anchor under the original
observation node/text/wire budgets. It scans at most 128 KiB of raw text,
including whitespace, and the original native node-visit limit still applies.
Large individual fields have the existing bounded normalization work ceiling;
any truncated scan, passage or result is explicitly incomplete. A miss therefore
means no match in the inspected content, never proof of absence from the page.
The runtime asset installation ceiling is 108 KiB after adding this recipe;
observation and provider disclosure budgets are unchanged.

Each result is tied to its actual semantic source key. Separate runs from the
same source are never stitched across nested sources, omitted content or privacy
boundaries. The fresh forest carries no action operations, navigation URLs,
editable values or invented parent relationships. Shared Rust verifies exact
query/scope/predecessor, frame and snapshot generation, source shape, source
count and text ceiling before model delivery. Previous action refs retire as
for every progressive snapshot. Work retains bounded public read-only evidence
for terminal mapping under the [retained evidence contract](agent-work-retained-inspection-evidence.md).
Search results can themselves supply current
source refs for narrower inspection; the original region anchor remains
available for another explicitly requested search.

Terminal extraction uses the current acknowledged observation plus admitted
same-document retained evidence. If the combined authorized inventory has no
source fragments and the trusted schema requires a
field, the controller closes with `NoExtractionEvidence` before sending or
counting a mapper request. Zero sources cannot support a required value. This
is an incomplete workflow outcome, not evidence that the requested facts do
not exist. Optional-only schemas retain their existing empty-result contract;
invented source refs remain rejected by normal extraction validation.

Deterministic coverage includes saturated initial inventories, offscreen
rendered evidence, inline quote fidelity, region/privacy/frame boundaries,
secret withholding, query and raw-scan bounds, Unicode output ceilings,
complete versus incomplete misses, and substituted query/source authority.
This capability does not qualify hidden disclosure controls or retained-page
action execution; those require their own effect and native ownership join.

## Recovering an invalid inspection scope

A model can request a well-formed `text_search` against a heading after a
`surrounding_text` capture. A heading is not a tree-search boundary. The
controller now returns an authenticated `invalid_snapshot_scope` tool result
and lets the model select another operation within the original call,
operation, token and time budgets. Unknown current refs and unsupported
inspection scope classes use the same pre-dispatch refusal. No rejected
capture runs, no generation advances, and the trusted capture timestamp and
retained evidence remain unchanged. Repeated invalid choices stop at the
existing limit, preserving the terminal mapper reservation.

`AgentProviderObservationResolution` separates an admitted capture checkpoint
from a move-only refusal bound to the exact settled proposal and observation.
Configuration, baseline, lineage, stale-frame and expansion-limit failures
remain terminal. Recovery does not turn a heading into its parent region or
silently issue a replacement search.

The refusal uses the existing OpenAI Responses whole-request-counted transport
and the original authentication. Its stateless body contains the current
observation exactly once, the approved objective and policy-bound progress,
the exact rejected call with its provider replay, and a compact content-free
error. Prior obsolete tool replay is discarded as with progressive capture
checkpoints; model-call budgets are not reset. The error reports
`executed:false`, `observation_unchanged:true`, and fixed guidance about eligible
scope roles. It cannot carry arbitrary host instructions or claim new page
evidence. Other provider/accounting combinations retain their explicit refusal
until their equivalent whole-input path is qualified.

The native product journal emits `InspectionRefused`. Deterministic shipping
controller coverage reproduces navigation → surrounding heading → invalid
search → refusal → cited extraction, also exercises an unknown ref and
repeated invalid choices, and checks exact native capture counts, accounting,
stateless call correlation, one observation per request and clean shutdown.
Core tests additionally reject baseline/config substitution and retain valid
scope admission for both provider correlation codecs.

## Inspection progress is not navigation progress

Progressive inspection is a same-document capability and therefore retains its
own bounded host checkpoint independently of route state. An OpenAI
provider-exact continuation can receive up to 4 KiB of content-free inspection
history even when the task has no navigation authority. Route checkpoints keep
their separate policy binding and validation. Neither checkpoint is page
evidence or a substitute for current semantic references.

Each accepted capture records only its scope class, snapshot generation, node
count, completeness, optional text window, and private stable native anchor
key. Before delivery, private keys are remapped to references that actually
exist in the current observation; absent keys become null. Page strings and old
references are never retained in this history. A refusal preserves the exact
history and current observation without advancing native generation.

The engine refuses a subtree request when the same stable subtree was already
captured, including after `snapshot(initial)` assigns it a different `@ref`.
A different descendant remains eligible. The current-subtree check also lives
in semantic observation construction, so provider correlation shape cannot
bypass it. Rejected scopes reach the model as `invalid_snapshot_scope` without
native capture or retained-evidence mutation.

Retained read evidence is admitted only after a snapshot proposal passes scope
resolution and immediately before its real replacement capture. A rejected
proposal therefore cannot evict useful prior evidence. Decision-call budgeting
applies to every progressive/read/action/extraction Work profile, not only
navigation discovery, and reserves the final mapping call mechanically.

The first authenticated same-page qualification used a disposable workspace
with manual provider-trace review.
