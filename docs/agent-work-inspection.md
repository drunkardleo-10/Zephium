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
Real application measurements are recorded separately in the
[M6 engineering record](../eval/agentic-browsing/m6-production-qualification.md).
