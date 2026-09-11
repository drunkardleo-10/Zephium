# Work frontend context — parallel implementation

Prepared 2026-09-11. This is context for the frontend agent while the runtime
agent continues implementation. It is not a runtime delivery, a branch handoff,
or a declaration that Work is ready for integration.

## Coordination and immediate answer

**Yes: initial V1 Rust-owned Work projection, execution command, signal, artifact,
and evidence types exist, with six JSON fixtures.** They are in the runtime
working tree at `/Users/crynta/Dev/Zephium`. At the time of this note, that tree is
on `main`. Commit `fd2d43d` (`Add durable Work execution and native browser
artifacts`) contains the initial contracts. Commit `68d28d51` contains the
synthesis and direct primary/child extension. The following runtime fixes and
live qualification are described below; there is no handoff branch to pull yet.

Continue frontend foundation and Work presentation work in your existing stream.
The runtime agent will keep implementing here. The user will push the frontend
changes when ready; after the runtime work is ready, we will deliberately pull,
reconcile, and build the integrated Work mode. Do not merge or replace either
stream as a consequence of this note.

The contract below gives enough shape to develop components and provisional
fixtures without inventing execution semantics. It is an initial versioned
contract, not a final freeze. Generated Work TypeScript exports and the complete
frontend transport remain integration work. If your checkout does not contain
these files, use this note as reference; do not treat their absence as a reason
to invent a competing backend schema or block all shared UI work.

## Sources of truth

Paths below are relative to `/Users/crynta/Dev/Zephium`:

| Path | Owns |
| --- | --- |
| `docs/product-system.md` | Product direction and Browse + Work scope |
| `docs/work-runtime-continuation.md` | Runtime architecture, current implementation, qualification limits, remaining work |
| `crates/zephium-ipc/src/work.rs` | `WorkProjectionV1`, `WorkCommandV1`, `WorkSignalV1`, artifact/evidence re-exports |
| `crates/zephium-core/src/work/mod.rs` | Work authoring snapshot, plan, questions, review requirements, revisions |
| `crates/zephium-core/src/work/ids.rs` | Work, plan, node, execution, attempt, command, artifact identities |
| `crates/zephium-core/src/work/runtime.rs` | Execution specification, facts, statuses, usage, command intents, receipt |
| `crates/zephium-core/src/work/artifact.rs` | Seven semantic artifact kinds, presentation hint, citation and evidence preview |
| `crates/zephium-app/src/actor/mod.rs` | Rust `Handle::work_command`, `work_projection`, `work_evidence`, `work_document` entry points |
| `crates/zephium-app/src/work_authoring_intent.rs` | Existing Rust authoring intents; not yet a versioned frontend command envelope |
| `crates/zephium-app/src/work_runtime.rs` | Live attempt ownership and transient observer; host implementation, not a renderer API |
| `crates/zephium-ipc/fixtures/work-approved-v1.json` | Complete example of a projection with an approved execution |
| `crates/zephium-ipc/fixtures/work-approve-command-v1.json` | Matching approval command example |
| `crates/zephium-ipc/fixtures/work-research-v1.json` | Real completed child/primary research, two documents requiring review |
| `crates/zephium-ipc/fixtures/work-research-evidence-v1.json` | Historical evidence previews matching both research artifacts |
| `crates/zephium-ipc/fixtures/work-cancelled-v1.json` | Real cancellation after native model admission, no artifacts |
| `crates/zephium-ipc/fixtures/work-failed-v1.json` | Real refused native navigation with settled failed attempts |

Rust/Serde definitions and their validators are authoritative. The IPC tests
deserialize these fixtures, validate their contracts, reject worker settlement
through user IPC, and register the types with Specta. This is not yet a complete
generated TypeScript package, JSON Schema bundle, or wired browser transport.

## Product model to preserve

Browse remains an excellent ordinary browser independently of Work. Work is a
persistent environment for accomplishing a goal, with visible responsibilities,
dependencies, questions, resources, approvals, evidence, and artifacts. Chat is
one way to express intent; it is not the aggregate or the execution state store.

A Work belongs to a regular profile and has its own durable identity. Tabs,
windows, native browser contexts, and canvas cards have different identities.
Attaching or displaying a resource must not silently copy data across profiles.

The spatial surface, node cards, inspector, rich editor, and other components
project semantic objects. Rust owns what a plan, execution, approval, attempt,
and result mean. View layout, selection, zoom, expansion, dragging, and temporary
editing state can remain frontend concerns behind a separate view-state layer.
Do not add geometry or editor implementation JSON to the runtime artifact types.
Persistence for any shared view state will need its own explicit contract.

Browser resource previews are descriptive cards. A field containing a URL does
not create a native context or authorize execution. Prefer a focused resource
surface following the product direction; do not assume a live WebView for every
canvas node.

## Wire conventions

- Work-related identities and profile IDs cross the boundary as canonical ULID
  strings. Keep the different identity roles distinct in application code.
- Revisions are canonical positive decimal **strings**, such as `"3"`. Do not
  parse them through JavaScript `Number` or compare them lexicographically.
  Equality can use strings; ordering needs precision-preserving handling.
- The outer runtime projection, command, signal, artifact, and evidence preview
  currently use `version: 1`. The nested authoring snapshot independently uses
  `schema_version: 2`. These versions describe different contracts.
- Field names and enum values use the exact snake_case Serde spelling. Tagged
  unions use a `kind` field. Optional fields serialize as `null` in these shapes.
- Budgets and usage counts are bounded integers; `cost_micro_usd` is millionths
  of a US dollar. Evidence `source_bytes` and chart point `value` are strings.
- Unknown versions need an explicit unsupported state. Do not silently coerce
  them into a known shape or preserve opaque fields as executable content.

## Durable projection and identity relationships

`WorkProjectionV1` is an alias for `WorkRuntimeProjection`:

```text
{
  version: 1,
  work: WorkSnapshot,
  executions: WorkExecutionFact[],
  interrupted: WorkExecutionId[]
}
```

The snapshot contains:

```text
{
  schema_version: 2,
  id, profile, revision,
  lifecycle: "active" | "archived",
  objective, objective_author,
  objective_revision, context_revision,
  status: "draft" | "needs_input" | "plan_ready",
  plan: WorkPlanRevision | null,
  questions: WorkQuestion[]
}
```

Author values are `user`, `primary_agent`, `other_agent`, and `legacy_unknown`.
Attribution is descriptive history, not permission to act.

A plan revision contains `author`, `revision`, `basis_revision`, and
`draft: { id, nodes }`. Each node contains `id`, `objective`, `dependencies`
(node IDs), and `outputs`. Each expected output contains `name`, `description`,
and `review`. Dependency edges express required data/completion order, not
canvas positions or delegation. Array order is not execution order.

A question contains `id`, `prompt`, `options`, `answer`, `author`,
`answer_author`, `basis_revision`, `objective_revision`, and `state`.
Question states are `active`, `answered`, `superseded`, and `dismissed`.
Answers, answer author, and legacy revision attribution can be null. Superseded
questions remain history; do not display them as currently blocking input.

Each execution contains:

```text
{
  id, approved_revision,
  spec: { plan_revision, limits, nodes },
  status,
  attempts: [{ id, node, status, usage }],
  artifacts: WorkArtifactV1[]
}
```

An execution binds an exact immutable plan revision, independently of later
authoring history. `approved_revision` is the Work revision recording approval;
`spec.plan_revision` identifies the approved plan. Do not join historical
execution nodes against an unrelated current plan. Rust has a plan-history read
API; exposing that through versioned frontend transport remains to be done.

There is no persisted status field on a plan node. A node card may project its
state from the selected execution's attempts and dependencies, but must keep
that view derivation separate from durable facts. An unattempted node has no
attempt; avoid minting placeholder runtime attempt IDs.

## Status, cancellation, recovery, and accounting

Execution statuses:

| Value | Meaning for presentation |
| --- | --- |
| `approved` | Exact plan and limits approved; no attempt started |
| `running` | Execution has begun and is not complete; not necessarily an active worker at every instant |
| `cancel_requested` | Stop intent persisted; original running ownership still needs settlement |
| `completed` | Required outputs present, all attempts succeeded, and no output requires semantic/user review |
| `needs_review` | Required outputs present and attempts succeeded, with review still required |
| `cancelled` | Execution cancellation settled; preserve actual attempt/artifact history |
| `failed` | Terminal failed execution; inspect durable attempts rather than guessing from a signal |
| `interrupted` | Loss of the old owner explicitly acknowledged; this does not resume execution |

Attempt statuses are `running`, `succeeded`, `failed`, `cancelled`, and
`outcome_unknown`. Attempt success is distinct from correctness of a semantic
result and from human acceptance.

`interrupted` at the projection root lists executions whose old Store incarnation
has no live authority. A read can therefore contain historical `running` facts
and an interruption indication together. Display interruption prominently; do
not restart a spinner as though the worker were alive or automatically resume
from decoded state. Reading history does not rewrite it. An explicit
`acknowledge_interruption` command can record the terminal acknowledgement and
unknown attempt outcomes.

Usage is either null or `{ model_tokens, cost_micro_usd, operations, accounting }`.
`accounting` is `exact` or `conservative_reservation`. Null means unknown or not
yet settled, never zero. The current browser adapter charges the approved
conservative ceiling; do not label it exact provider spend. Unknown outcomes
retain their reservation. Progress text cannot refund or recalculate a budget.

Cancellation is an intent, not immediate proof that work or native effects
stopped. A late original result may remain in the facts even when aggregate
cancellation wins. Keep that history. Current retries require fresh approval of
a new execution; there is no frontend `retry_attempt` or `resume` command.

## Commands and transport boundary

The current execution command envelope is:

```text
{
  version: 1,
  work: WorkId,
  expected_revision: WorkRevision,
  command: WorkCommandId,
  intent:
    { kind: "approve", spec: WorkExecutionSpec }
    | { kind: "cancel", execution: WorkExecutionId }
    | { kind: "acknowledge_interruption", execution: WorkExecutionId }
}
```

These are the only V1 runtime command intents today. Approval checks the exact
current Work revision and exact plan specification. `plan_ready` alone cannot
authorize execution. Active execution blocks authoring changes and deletion.

Each logical command has a stable ID. After a lost reply, retry the **same ID,
same expected revision, and same operands**. Store returns the original receipt
and current projection; it does not execute twice. Reusing an ID with changed
operands is a conflict. A definitively rejected stale command needs a fresh read
and deliberate re-evaluation; do not silently approve a changed plan.

The Rust receipt is `{ command, applied_revision, execution }`. A successful
command reply includes it alongside the projection. That receipt revision can
precede the current projection revision on replay. The complete serialized
response/error envelope and transport command names are not frozen yet:
`WorkReply` and `WorkError` are currently Rust application/Store contracts,
not a frontend JSON envelope to reproduce by guesswork.

Rust `Handle` entry points take an explicit displayed `profile` for runtime
commands, projection reads, and evidence reads. Shell validates the selected
regular profile; merely supplying a profile string grants no access. Keep
frontend caches and pending responses scoped by profile and Work. Discard
responses/signals that no longer belong to the displayed context.

Authoring already supports Rust intents for create/read/list, plan history,
objective edits, opening/answering/dismissing questions, replacing a draft,
archive/restore, deletion, and history compaction. Rust mints new domain
identities and sets user attribution. The complete versioned authoring IPC,
model-planning command flow, and error mapping still need integration. Do not
append invented authoring variants to `WorkCommandV1`.

Worker start, settlement, artifact publication, usage submission, and resource
ownership are host-only. They cannot be submitted by UI JSON, model output, or
restored fixtures. There is currently no artifact edit, checkbox mutation,
semantic result acceptance, or consequential-effect approval command in this
V1 envelope. Such interactions can be designed, but backend persistence and
authority require an explicit future contract.

## Approval specification

Global and per-node `limits` contain `model_tokens`, `cost_micro_usd`,
`operations`, `timeout_seconds`, and `max_workers`. Each execution-spec node is
`{ node, parent, capability, limits }` and must match a node in the approved plan.

Capabilities currently have these wire forms:

```text
{ kind: "public_browse", scope: {
    start_url,
    routes: [{ origin, path_prefix }],
    max_hops
} }
{ kind: "synthesize" }
{ kind: "coordinate", scope: {
    start_url,
    routes: [{ origin, path_prefix }],
    max_hops
} }
```

Public browsing scopes use exact HTTPS origins and bounded path prefixes.
Redirects do not widen scope. Approval presentation should show the proposed
scope and limits; it must not infer permissions from the objective or URLs in
artifacts. Rust validates coverage, bounds, and non-widening relationships.

The `parent` field describes a direct delegation relationship independently of
dependencies. A parent must have `coordinate` capability. Its children cannot
widen its approved scope or limits. Rust validates the combined dependency and
parent-completion graph to reject cycles. The original live coordinator can
admit direct children; standalone Begin still refuses them. A persisted parent
ID never creates live authority. The current application driver supports one
primary and direct children, with host-selected approved dispatch order.

`synthesize` now has a bounded model executor that produces typed artifacts from
approved dependency data. A real checklist run passed artifact validation and
Store reopen. The coordinator uses the same executor for the primary's final
output after original child publication receipts arrive. These move-only
receipts are private runtime ownership, not renderer values or restart tokens.
The provider's internal structured-output encoding does not change the flat
`data: { kind, ... }` artifact contract below.

## Semantic artifacts

Every artifact uses this immutable envelope:

```text
{
  version: 1,
  id, execution, node, attempt,
  output, title,
  data: WorkArtifactDataV1,
  evidence: [{ extraction_id, source_id }],
  review: "mechanical" | "source_mapped_needs_review" | "user_acceptance",
  presentation: "automatic" | "compact" | "expanded"
}
```

`output` matches a named expected output of the exact plan node. Artifacts are
bound to a successful original attempt and exact execution. The current contract
does not include editable versions or per-paragraph citation offsets.

All seven `data` variants are defined and validated:

| `kind` | Additional fields |
| --- | --- |
| `document` | `paragraphs: string[]` |
| `table` | `columns: string[]`, `rows: string[][]` |
| `comparison` | `criteria: string[]`, `alternatives: { name: string, values: string[] }[]` |
| `chart` | `x_label: string`, `y_label: string`, `series: { name: string, points: { label: string, value: string }[] }[]` |
| `checklist` | `items: { text: string, completed: boolean }[]` |
| `evidence_collection` | `summary: string` |
| `browser_resource_preview` | `title: string`, `url: string`, `summary: string` |

Table row widths match columns. Comparison values match criteria order. Chart
values are strings validated as finite numeric values; retain the original
string even if a chart library needs a numeric rendering value. Checklist
completion is part of the immutable artifact payload today, not a wired user
toggle. Evidence collections use the envelope's evidence array. Browser preview
URLs are HTTPS descriptive data, not native resource handles.

Current bounds include 64 artifacts per execution, 64 evidence links per
artifact, and 32 KiB total text across each data payload. Documents and
checklists allow up to 128 entries; tables up to 16 columns and 128 rows;
comparisons up to 16 criteria and 32 alternatives; charts up to 8 series and
128 points per series. The validators remain authoritative for detailed bounds.

`presentation` is a semantic display hint, not required geometry. Use native
Zephium components and text-safe rendering. The payload contains no privileged
HTML, JavaScript, CSS, Tiptap JSON, or model-selected renderer code. Rich editing
can adapt to the editor layer, but changing an editor document must not silently
overwrite the persisted runtime artifact.

The three `review` values name requirements, not fulfilled review states.
`source_mapped_needs_review` establishes source attribution and requires human
review; it does not certify an interpretation. `user_acceptance` does not mean
the user accepted anything. Avoid a generic verified badge inferred from either.

Only the source-mapped **document** path has been exercised through the real
native browser adapter so far. The other six kinds are semantic contracts ready
for renderer development, not evidence of seven working model adapters.

## Evidence and citations

A citation is `{ extraction_id: WorkArtifactId, source_id: number }` with a
nonzero bounded source ID. It identifies a source inside an original persisted
extraction; it is not a URL supplied by a model. Note that extraction IDs and
semantic artifact IDs currently share the Rust `WorkArtifactId` type but have
different roles. Do not substitute an output artifact's ID for an extraction ID.

On-demand evidence reads return:

```text
{
  version: 1,
  link: { extraction_id, source_id },
  origin, role, text,
  truncated: boolean,
  source_bytes: string
}
```

Store verifies Work/profile membership, extraction integrity, and exact source
identity before returning up to 8 KiB of historical UTF-8 source text. Display
truncation and the historical nature of the quote. Source content is untrusted
text. A citation popover or evidence inspector is appropriate; selecting it does
not restore browser authority or prove the page is still current. There are no
source offsets or complete source-page DOM in this preview contract.

## Transient activity

`WorkSignalV1` contains `version`, `profile`, `work`, `basis_revision`,
`execution`, `node`, `attempt`, and `activity`. Activity values are:

```text
planning, delegating, reading, comparing, producing_artifact,
waiting_for_approval, waiting_for_human, cancelling, finishing
```

This is one replaceable transient value, not a durable event log or a reliable
ordered stream. There is no percentage, sequence number, transcript, or token
delta. Render only when identity and durable revision match the relevant current
attempt; clear stale activity on projection/context changes. A missing signal
does not imply failure, and `finishing` does not imply completion. Some values
are reserved vocabulary for paths still being implemented.

The Rust observer owns no worker, browser resource, or idle timer. Transport and
subscription wiring remain pending. Keep eventual observation demand-driven;
do not introduce perpetual polling or native context retention just to animate
an idle Work card.

## Useful frontend work now

Continue the established design system and shared frontend architecture. For
Work-specific development, the following can proceed behind a replaceable
transport adapter:

1. Work home and objective/plan/question presentation, with clear separation
   between authoring status and execution status.
2. Node and dependency presentation keyed by real domain identities, with view
   state kept separate from the semantic projection.
3. Renderers for all seven artifact kinds, plus evidence preview, review-required,
   empty, loading, unsupported-version, and unavailable states.
4. Approval scope/budget presentation, cancellation pending, interruption,
   unknown outcome, and conservative-usage presentation.
5. Profile-scoped state and stale-response handling; full-projection replacement
   rather than deriving durable state from animation events.
6. Component fixtures based on these shapes. Keep locally constructed fixtures
   explicitly provisional until validated against the integrated Rust contract.

The approval examples and four files derived from real public native runs are
listed above. Rust tests validate their plan/execution/attempt/output
relationships and match the research artifacts to their historical evidence.
These are renderer data, never restored live authority. The cancelled fixture
preserves a failed primary with zero usage and a cancelled child with conservative
usage; it must not appear as successful output. The research fixture retains
`needs_review` even though both attempts succeeded. More artifact kinds and
recovery cases remain to be added; this is not a complete fixture suite.

Keep unavailable actions behind the adapter/capability boundary rather than
pretending they persisted. Avoid designing a second scheduler, policy engine,
artifact store, approval mechanism, or resume protocol in frontend state.
Rendering components should not need to know provider credentials, engine
handles, native fences, raw page observations, or model reasoning/transcripts.

## Runtime progress and remaining integration

Implemented in the current runtime tree: persistent authoring and model-backed
planning; exact revision-bound execution approval; durable attempts and command
receipts; dependency/output checks; cancellation and interrupted-owner history;
typed semantic artifacts; on-demand evidence reads; and an application-owned
public browser adapter using the original native resource lifecycle.

A real public workflow has passed: a Luna-generated plan, explicitly bounded
qualification approval, model-directed native SQLite documentation research,
source-mapped document publication after original resource closure, Store reopen,
and successful historical citation reads. It ended in `needs_review`. This
proves that public-browser vertical only. The local ignored report is
`target/work-runtime-proof/public-research.json`; that single-worker report is
not a release qualification of the whole runtime. The newer combined run has a
committed projection fixture listed above.

The runtime tree also implements direct primary/child ownership through the
existing orchestration core, exact fresh publication joins, bounded dependency
handoffs, and model synthesis. Deterministic tests cover successful child-to-primary
production and Store reopen, missing/unknown children, receipt substitution,
duplicate dispatch and cancellation. A real standalone synthesis run produced
five incomplete release-checklist items, retained conservative usage, and reopened
its artifact from Store (`target/work-runtime-proof/synthesis.json`). Public
qualification explicitly enables provider response storage; normal requests use
`store: false`.

The combined native worker/primary synthesis workflow now passes. It exposed and
fixed two runtime defects: navigation accounting counted only the first two
hops, and the pinned runtime read lane omitted historical evidence reads. The
browser also receives explicit guidance about expanding its initial viewport
capture. A fresh run published the child's cited findings and the primary's own
explanation, closed original native resources and reopened both artifacts and
citations from Store. The report is
`target/work-runtime-proof/coordinated-research.json`; output remains
`needs_review`. The stored primary input contains its responsibility, the child
document and bounded historical source previews, without a browser transcript.
Separate real cancellation and refused-navigation runs also closed resources and
reopened their durable facts; see the continuation document for exact reports.

Remaining runtime work includes dynamic primary decisions, selected decision and
resource assignments, deeper/parallel worker execution, consequential-effect
approval and settlement, user acceptance/editing, and durable planning/model-call
accounting. The native browser path requires original foreground ownership and
should not be presented as background multi-browser execution. The combined
success proves the direct-child join with host-selected dispatch, not a complete
primary scheduling policy or arbitrary-site reliability. The previous complete
runtime regression checkpoint passed 1,149 tests (four opt-in qualifications
ignored), plus nine provider boundary tests. The follow-up fixes passed 619 core,
75 controller and 12 application Work tests, plus strict runtime Clippy.

At integration, we will reconcile the frontend stream with the then-current Rust
contracts, generate the Work TypeScript types, finish versioned authoring and
response/error envelopes, wire projection/command/evidence/activity transport,
validate fixtures against Rust, and exercise actual Work interactions. Until
then, this document is the shared semantic reference for parallel development.
