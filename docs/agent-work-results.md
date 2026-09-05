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

The actor restricts this task to one advertised tool: `extract` with the initial
scope and exact schema identity. Native actions, navigation and observation
expansion are not advertised or admitted by this task. After a fresh actual
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

The result body is **memory-only**, not a durable artifact. Durable `Succeeded`
records execution closure, not artifact-body persistence. Process exit before
the consumer takes the result loses that body; restart never recreates it by
replaying a provider request or native operation. This distinction is explicit
in the application API and qualification metrics.

## Qualification and next seam

The excluded macOS `--live-public-luna-work-extraction-inspectable` qualifier uses
the actual composition, shell, EngineHost-owned extension-free page and exact
shared SQLite Store. Its trusted task checks ten distinct public Wikipedia
language-link names against their cited native observation before completion.
The delivery consumer checks the owned values and provenance again, then
requires durable success, focus isolation and clean application-owned teardown.
Only this explicit public mode retains provider logs. See the [M6 evidence](../eval/agentic-browsing/m6-production-qualification.md).

Next is durable artifact delivery: a separate private content store and typed
artifact identity, with an exact terminal/result publication transaction or
recoverable outbox, bounded retention and restart classification that never
replays execution. Result content must not enter the existing content-free Work
journal or audit rows. User-facing task authoring, initial-scope expansion,
navigation, multi-page research, new native lifetimes and parallel runs remain
separate authorities. Default Browse dependencies and scheduling are unchanged;
this is structural evidence, not battery, CPU/RAM or broad-site qualification.
