# Bounded Work extraction results

The opt-in production Work application can now return a useful result, not only
terminal execution facts. `AgentWorkExtractionTask` accepts a trusted, closed
schema and account scope. It uses the existing `AgentBrowserSession`, runtime
worker, native Work context, provider transport, policy accounting, audit ledger
and durable application coordinator. There is no second extraction worker or
automation stack, and no UI/IPC authority is introduced.

This vertical was selected because the previously qualified product path could
manipulate form state but could not deliver structured information. Initial
public-data extraction reuses the already implemented semantic read, extraction
and committed-output authorities without inventing navigation, restart or native
port reuse. See [composition](agent-work-composition.md) for trusted admission.

## Exact contract and disclosure

The trusted caller supplies bounded field names, types and limits through
`SemanticExtractionFieldSchema`. Supported values are text, Boolean, unsigned
integer and flat text lists. Schema identity `1` is scoped to this run; the model
cannot register a schema or substitute an executable task. The existing approved
objective describes the mapping requested by the user. It is not effect authority.

By default the actor restricts the generic extraction task to one advertised
tool: `extract` with the initial scope and exact schema identity. Native actions
and navigation are never admitted by this task; subtree capture requires the
separate trusted opt-in described below. After a fresh actual
native observation, the session validates the current frame cohort and consumes
the exact provider tool continuation. The read uses `PublicOnly` sensitivity,
at most 128 readable fragments and 32 KiB of read content, further restricted by
the 16 KiB combined encoded schema/read preflight. Insufficient evidence or a
budget refusal fails closed; it does not trigger a larger disclosure or retry.

The mapping call binds the provider's strict output schema to the exact trusted
field names, types, schema identity and bounds. The same projection semantics
apply to the existing provider protocol adapters; provider constraints improve
generation but are not validation authority. The selected transport still
counts the entire request, charges the original policy and enforces the same
run deadline/turn ceilings. Production/BYOK requests remain stateless.
The output envelope follows the [official structured-output subset](https://developers.openai.com/api/docs/guides/structured-outputs):
an object root, closed object properties and nested field alternatives.

One purpose-bound collector retains output only after the exact extraction input
commits. It releases a result only after the matching provider call completes,
is priced and policy-settled, contains no tool output, and passes the existing
Rust schema, field order, text/secret, sensitivity and exact-source validation.
Wrong types, foreign citations, incomplete output and accounting failures do
not become results. Ordinary assistant text is still discarded by the driver.

## Content is not diagnostic data or execution authority

`SemanticOwnedExtractionResult` moves the admitted fields and retains each cited
native read fragment once. Citation edges are bounded, deduplicated and guarded
against foreign source spans. The result survives teardown of the observation
without keeping a page, provider body or credential alive. It retains capture,
observation and historical frame/reference provenance. Historical references
do not restore native authority and cannot be used as fresh action targets.

The existing hard extraction bounds remain 64 fields, 256 scalar/list-item
values, 1,024 citation edges and 64 KiB of mapped text. The production read limit
further bounds unique quoted source content. There is one result slot, not an
unbounded artifact queue. Diagnostics and event queues contain only typed states,
counts, correlations and timings; result/source `Debug` paths do not print content.

All results are explicitly `ModelMapped`. Core validation proves structural
admission and citation provenance, not that an inference is factually true.
The generic trusted extraction task completes that structural contract. A trusted
product task can additionally inspect the admitted result in `accept_extraction`;
model text never supplies this predicate. The public qualifier independently
compares each returned value with its exact cited native source text.

## Verified actions followed by one result

A trusted `AgentWorkTask` can explicitly opt into `allows_actions_before_extraction`
and supply the same single schema. Schema and mode are frozen at admission;
changing either is a contract refusal. The provider configuration advertises only
the existing bounded `locate`, snapshot-verifiable `act`, and schema-1 `extract`
tools. That exact configuration remains bound to the entire continuation lineage.
An extraction schema alone still enables no actions, and the combined profile
grants no effect authority beyond the original manifest and trusted assessment.

The task evaluates the initial observation and every independently verified
adjacent post-action observation. `Continue` permits more actions but refuses
extraction. `ReadyForExtraction` certifies the trusted action postcondition and
permits extraction but refuses further actions. A combined task cannot declare
`Complete` instead of returning a result. Readiness may already hold initially;
neither a fixed action count nor a model response is completion authority.
Bounded, read-only locate turns remain available in either phase and consume
the same model-call budget; they cannot dispatch an action after readiness.

The extraction proposal and mapping call consume the same bounded session, policy,
transport, accounting and audit owners as the actions. The eight-call ceiling
includes locate, actions, the extraction proposal and mapping; no budget resets
at the phase boundary. Premature extraction, late actions, unauthorized scope expansion and
schema mismatch are typed refusals with no retry. Mapping is bound to the exact
current observation, its frame cohort and refreshed capture timestamp, not the
pre-action baseline. `accept_extraction` must then explicitly accept the validated
result before the original closure/publication path can expose it.

This enables a useful action-to-result workflow without a second controller,
worker, result queue or native authority. It does not enable navigation, another
extraction round, mutation replay or model-authored tasks.

## One fresh native subtree read

`AgentWorkExtractionTask::with_subtree_extraction()` or the trusted task's
`allows_subtree_extraction()` explicitly enables initial scope or one subtree
capture. The flag is frozen alongside the schema and action mode. Both provider
serializers advertise only initial/subtree extraction with schema 1 and canonical
opaque refs; scoped read-only tasks also advertise locate, never act.

The exact settled extraction continuation validates the selected ref against its
committed predecessor observation and current frame cohort. Rust constructs the
existing bounded native subtree request, then the same runtime/browser port
captures it once. Native correlation, context/frame/origin, anchor, parent,
observation and snapshot generations must all join. A result rooted at another
private stable node key cannot be mapped. Callback refusal, stale capture,
takeover or renderer loss cannot fall back to the initial read or retry.

The native runtime traverses that anchored subtree, including rendered content
omitted by initial viewport filtering, under the unchanged 128-node/16-KiB text
budget. Read and schema encoding retain their existing bounds. The read guard
binds the fresh source plus exact acknowledged predecessor and target. The sealed
provider draft carries that target through policy admission: its original
committed taint cohort must exist and match the account and origin; fresh source
taint is keyed by the read guard. No full-observation acknowledgement is minted,
and mapping cannot create another browser-tool continuation or action authority.

Combined tasks must first satisfy their existing trusted action-readiness
contract. The scoped result cites the additional capture, not the last action
snapshot. `accept_extraction` still decides whether that evidence satisfies the
product task. There is one mapping/result, no separate read worker, no larger
budget, and no persistence/publication bypass. Table/frame/region expansion,
multi-page collection and cross-document navigation remain unimplemented host
adapters.

## Publication, cancellation and recovery

The original runtime, native-resource, provider, policy and durable audit closure
must succeed before `AgentWorkSuccess` can own the result. The application then
requires its original clean lifecycle and exact immutable durable terminal CAS
acknowledgement. Only afterward can `AgentWorkApplicationHandle::take_extraction`
move the result once. All earlier, uncertain, review and recovery phases refuse
the handoff without consuming it.

Cancellation or takeover during either provider count or stream uses the same
sealed runtime stop lane and native revocation/reconciliation path as action
tasks. Partial mapping output is discarded while original provider/accounting
debt remains owned. Audit loss after a valid mapping retains the drained original
owners and an unpublished result; it cannot manufacture successful closure.

Results remain **memory-only by default**: `Succeeded` then records execution
closure, not body persistence. A trusted durable-profile extraction request can
now explicitly require [profile-owned artifact publication](agent-work-artifacts.md).
That intent is persisted before execution and requires an atomic body/terminal
transaction before handoff. Restart retrieval yields historical `ModelMapped`
data, never a provider/native replay or restored execution authority.

## Qualification and next seam

The excluded macOS `--live-public-luna-work-extraction-inspectable` qualifier uses
the actual composition, shell, EngineHost-owned extension-free page and exact
shared SQLite Store. Its trusted task checks ten distinct public Wikipedia
language-link names against their cited native observation before completion.
The delivery consumer checks the owned values and provenance again, then
requires durable success, focus isolation and clean application-owned teardown.
Only this explicit public mode retains provider logs. See the [M6 evidence](../eval/agentic-browsing/m6-production-qualification.md).

The `--live-public-luna-work-combined-inspectable` qualifier exercises the same
application path with three independently verified, unsubmitted public form-state
changes followed by a cited final-value result. Both the trusted task and the
application delivery consumer independently compare that value with its exact
post-action native source. It adds no production scenario or special authority.

The artifact vertical adds bounded typed private bodies and exact immutable
publication on the same Store actor, separate from content-free journal/audit
rows. The scoped public qualifier adds targeted native read evidence described
in M6; it does not qualify off-viewport discovery on public sites. User-facing task authoring,
navigation, multi-page research and parallel runs remain separate authorities.
Explicit sequential native lifetimes require the original predecessor's exact
closure and a newly admitted run, not result references. Default Browse dependencies and scheduling are unchanged;
this is structural evidence, not battery, CPU/RAM or broad-site qualification.
