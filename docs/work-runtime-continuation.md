# Zephium Work runtime continuation brief

Status: current implementation handoff and delivery direction  
Backend baseline: `c88e6d1` (`Close bounded navigation grounding gaps`)  
Prepared: 2026-09-11

## 0. Purpose and authority

This is the starting document for the next Zephium implementation phase. It
explains the product being built, the production foundations already present,
the honest limits of current evidence, and the system that should now be built.
It is deliberately a direction and architecture brief rather than a sequence of
small prescribed tickets. The continuation agent is expected to inspect the
actual tree, reason independently, research when evidence is missing, and change
the proposed mechanism when it finds a better production design.

This document does not replace the established sources of truth:

- [`product-system.md`](product-system.md) controls product direction and
  first-release scope.
- [`security-model.md`](security-model.md) controls claims about currently
  enforced security guarantees.
- [`architecture.md`](architecture.md) controls the implemented browser and
  native-shell foundation.
- [`frontend.md`](frontend.md) controls the checked-in Svelte frame; the newer
  frontend/design-system work may live in a separate development stream until
  it is intentionally reconciled.
- [`agentic-browsing.md`](agentic-browsing.md) and the narrow `agent-work-*`
  documents contain detailed contracts and engineering history for the browser
  execution substrate.
- `eval/agentic-browsing/` contains evidence records. An evidence record proves
  only its named configuration and must not be generalized into a release
  claim.

When prose and code disagree about present behavior, code and tests win. When a
new feature would weaken an implemented trust, ownership, or lifecycle
invariant, revise the feature design rather than silently bypassing the
invariant. Use an ADR only for an expensive-to-reverse boundary; ordinary
implementation does not need ceremony.

## 1. Product direction

Zephium has two native ways of using the web:

**Browse** is a premium, private, lightweight browser that must be worth using
when every AI and Work capability is disabled.

**Work** is a persistent environment for accomplishing substantial goals with
people, agents, browser resources, tools, services, evidence, tasks, knowledge,
and artifacts.

The central product insight is that complex work cannot be represented well as
an opaque agent followed by a transcript. Complex work has structure,
parallelism, dependencies, resources, intermediate state, decisions, effects,
unfinished parts, and persistent outcomes. Zephium makes that state visible and
directly manipulable.

Language remains an efficient way to express intent, but chat is not the
primary product surface. The agent is an actor inside Work, not the interface.
The environment is the representation of the work itself.

The product must preserve these properties:

- normal browsing carries no novelty tax;
- Browse remains excellent and independently disableable from AI and Work;
- Work follows clarify, understand, plan, approve, execute visibly, and persist;
- plans, tasks, resources, evidence, questions, approvals, and results have
  durable product identity instead of existing only inside a message;
- the user can inspect, redirect, stop, approve, reject, or take over without
  racing an agent for control;
- browser pages are first-class resources, but tabs and embedded WebViews are
  not the Work data model;
- agents populate typed native Zephium components rather than injecting
  arbitrary privileged HTML, CSS, or JavaScript;
- local models, BYOK providers, and Zephium-hosted AI use one architecture and
  one set of capability, policy, tool, result, and projection contracts;
- local-first privacy, zero product telemetry, low idle overhead, and bounded
  resource use are product requirements rather than later optimizations.

The first release is the real Browse + Work product on macOS and Windows. Linux,
cloud browser execution, mobile continuation, multiplayer collaboration, teams,
and capability packs are later expansions. The architecture must leave honest
ports for them, but the current phase must not build their infrastructure.

## 2. Why the current sequencing is correct

The frontend and runtime should meet through typed product contracts, not invent
each other from opposite sides.

The separate frontend stream has established a stronger Svelte 5 architecture,
design system, components, state conventions, and early Work units such as
tasks/nodes. That work is valuable, but the final Work canvas should not decide
what a run, plan, approval, task, resource, or result means. Rust owns those
semantics.

The backend stream has now proven the difficult browser actor beneath Work. It
can move from a natural-language objective through a real model and real native
WebView operations to a source-bound, durable terminal result. The correct next
center of gravity is therefore the **full Rust Work runtime and AI system**:
the durable Work aggregate, intent and plan lifecycle, approval compilation,
execution orchestration, agent/tool scheduling, evidence and artifacts,
questions and human intervention, persistence, recovery, and frontend
projection.

Once those contracts exist as a coherent vertical system, the product
integration agent can connect the frontend foundation and the runtime without
making the canvas authoritative or wrapping the browser actor in ad hoc IPC.

## 3. Current implementation checkpoint

### 3.1 Browser foundation

Zephium already owns a serious WebView-based browser architecture rather than a
mock shell. The native engine, Rust shell, profile and page lifecycle, blocker,
storage, and extension architecture exist. Native Rust ad/tracker blocking and
substantial Chrome-extension compatibility have been validated separately,
including difficult macOS extensions. Windows parity work continues in another
stream. Exact guarantees at a revision remain those enforced by the current
tree and `security-model.md`.

The architectural advantage is not that Chromium could never reproduce Work.
It is that Zephium controls browser-context identity, resource lifecycle,
visibility, suspension policy, profiles, history, Work ownership, and native
integration instead of retrofitting everything onto a tab-only product model.

### 3.2 Agentic browser kernel

The current Rust implementation is a production-shaped bounded execution
kernel, not a Playwright/CDP wrapper and not a model-controlled JavaScript
console.

The shared provider protocol defines a closed typed browser vocabulary:

- `navigate`, `back`, `forward`, and `reload`;
- `snapshot` and `locate` over compact semantic state;
- `act` through verified fixed native/page recipes;
- `wait` over bounded typed conditions;
- `read` and schema-bound `extract`;
- bounded viewport `screenshot` for genuinely visual questions;
- `show_for_human` and the separately controlled human-resume boundary.

That vocabulary is a protocol ceiling, not a claim that every task receives or
that the current Work controller has live-qualified every name. The production
path dynamically projects only the task- and phase-supported subset. Navigate,
exact native Back, snapshot/locate, verified act, wait, read, extraction,
screenshot admission, and terminal human request have implemented Work paths.
Forward, reload, and in-run resume remain capabilities to integrate and qualify
only where the product runtime actually needs them; the present human handoff
correctly closes authority and requires trusted fresh successor admission.

Capabilities are projected by phase. A model does not receive every operation
merely because the runtime implements it. It acts on opaque, generation-bound
references from observations it actually received. Raw HTML, selectors, CDP,
DOM objects, credentials, cookies, arbitrary JavaScript, native handles, and
extension authority are absent from the model surface.

The semantic runtime keeps geometry and full native facts where Rust needs them
for freshness, hit testing, visibility, occlusion, and verification while
omitting wasteful coordinates and collapsed option inventories from ordinary
model input. Collapsed select options are resolved on demand. Page content and
screenshots remain explicitly untrusted data.

The controller binds each proposal to the approved manifest, plan-node lease,
profile, account attestation, origin/destination rules, data sensitivity,
effect class, observation generation, tool profile, budgets, deadline, and
current native state. The host independently verifies effects before success.
Navigation retires stale transcript/ref authority and requires a fresh native
observation. Human takeover and cancellation revoke automation authority.

Work-owned browser resources are distinct from ordinary tabs and from a run's
revocable execution lease. A retained resource can survive a completed lease;
the old provider continuation and action references cannot. Visibility and
promotion are presentation state, not a transfer to another browser system.

### 3.3 Execution, lifecycle, and persistence foundation

The current crates separate responsibilities intentionally:

- `zephium-agentic` contains the functional core: manifests, policy, semantic
  state/actions/results, browser-resource ownership, evidence, audit, model
  protocol contracts, supervisor topology, and bounded orchestration state.
- `zephium-agent-runtime` owns the bounded runtime worker, mailbox, cancellation,
  and execution lifecycle without embedding provider HTTP or product UI.
- `zephium-agent-controller` joins a trusted task contract, provider session,
  browser port, policy, verification, accounting, audit, and terminal result.
- `zephium-agent-model-catalog` fixes provider/model capabilities and pricing
  identities.
- `zephium-agent-provider-transport` performs bounded provider transport and
  credential loading; the runtime itself remains transport-neutral.
- `zephium-app` owns application admission, selected-profile binding, Store and
  Engine identity joins, durable state transitions, recovery, and projection.
- `zephium-work-composition` connects the real macOS desktop Engine/Store to the
  same application and controller path. Qualification features are explicitly
  excluded from release builds.
- `zephium-engine` owns the native agent browser port and platform WebView
  adapters; it does not become the Work product model.
- `zephium-store` persists the existing content-free execution journal and
  bounded extraction artifacts through the normal Store owner.

An existing `AgentWorkOrchestration` functional core already provides bounded
single-owner scheduling over an approved delegation topology, node attempts,
progress, cancellation, output references, and child-to-parent handoff. It is
important seed architecture, but it is not yet the full product runtime: it
starts no worker or model, owns no durable plan aggregate, is not wired through
application admission to browser/service workers, and intentionally cannot
rehydrate execution authority from a checkpoint.

Current Work persistence is also narrower than the product requires. It proves
durable admission/terminal CAS behavior, recovery classification, audit
delivery, and optional atomic storage of a bounded source-mapped extraction. It
does **not** yet persist the complete Work, plan, task/resource graph, questions,
approvals, view-independent outcomes, or orchestration state required by the
product.

### Product authoring slice after the handoff baseline

The continuation implementation adds a Rust-only durable authoring path through
`Handle::work_document`, the normal Store actor, and profile schema 14. Core now
owns the same canonical `WorkId` re-exported by agentic resources. Plain intent,
clarification questions/answers, bounded editable plan revisions, semantic audit,
CAS, history, and restart-safe full projections persist independently of runs.
See the product-authoring section of `agent-work-persistence.md` for the contract,
limits, migration decision, and tests.

This advances the authoring/persistence boundary only. Model planning, trusted
scope resolution, approval compilation, execution settlement into this aggregate,
resources/artifacts/tasks, generated IPC, and frontend integration remain to be
built. `plan_ready` is descriptive draft state and cannot authorize execution.
The browser actor and its qualification baseline remain unchanged. The checked-in
frame is the earlier browser frontend, not the separate Work design-system stream.

### 3.4 Frontend foundation

The checked-in frame uses Svelte 5, strict TypeScript, Vite, Tailwind CSS v4,
generated typed IPC, and native presentation boundaries. Rust is authoritative;
Svelte projects state and emits typed intent. The separate frontend stream has
advanced the architecture, design system, component/state foundation, and Work
units beyond the exact backend baseline named above.

Before integration, inspect and reconcile the actual frontend revision. Do not
assume this checkout contains that stream, duplicate its components, or redesign
it from an older file listing. Preserve the core boundary: frontend nodes are
views of Rust-owned Work concepts, not a second Work domain.

## 4. Evidence at this checkpoint

The browser actor is sufficiently proven to move the main engineering effort to
the Work runtime. That does not mean agentic browsing is release-qualified on
every site or platform.

At backend baseline `c88e6d1`, one immutable bundled macOS build ran the held-out
retained Back qualification three independent times with GPT-5.6 Luna at medium
reasoning. Luna received the objective, scope, initial page, and phase-appropriate
tools; it was not given a click route or answer. Across the three runs it chose
the relevant link, inspected the compatibility record, returned through exact
native WebKit history, extracted the release code from the restored current
page, produced source mapping, durably terminalized, retained a healthy reusable
resource until shutdown, and closed all owners cleanly.

The three runs completed in 5, 5, and 7 model calls; used 17,723, 16,872, and
24,721 input tokens; and took 20.6, 17.1, and 22.4 seconds. Their paths varied,
including optional locate/read/snapshot decisions, which is evidence of an open
workflow rather than one memorized tool sequence. Provider request retention was
enabled only in the public release-excluded qualifier (`store:true`) for manual
inspection. Production and ordinary BYOK calls remain stateless.

Earlier evidence additionally covers real Luna semantic actions, progressive
inspection, source-bound extraction, durable success/failure/review paths,
retained resources, real multi-page discovery, a read-only authenticated Notion
workflow, screenshots at the policy/controller boundary, human-request closure,
and exact macOS application/native lifecycle behavior. The records under
`eval/agentic-browsing/` state the precise limitations of each result.

Notable resolved risks include:

- collapsed select options and geometry no longer dominate model tokens;
- semantic observations and exact refs can drive real native input without CDP;
- the foreground retained WKWebView path has an actual application rAF witness;
- inactive Work pages use the intended throttled scheduling policy rather than
  relying on animation frames that WebKit may suspend;
- Work-owned WebViews can remain retained across execution leases;
- exact native Back works without URL guessing or granting stale refs;
- route, operation, token, cost, and model-call budgets are enforced and
  provider-visible tools disappear when the host cannot accept them;
- model-requested human handoff, cancellation, uncertain callbacks, audit debt,
  and shutdown remain typed terminal/recovery states rather than false success.

The latest focused checks are green: 20 provider-request tests and 45 controller
tests, plus a successful debug application bundle. These checks are a narrow
regression statement, not a replacement for the repository's broader gates.

## 5. Honest remaining gaps

There is no known fundamental macOS WebView, semantic observation, native Back,
provider transport, or lifecycle blocker preventing Work. The primary gap is
now product runtime composition.

Today, a `TrustedWorkRequest` still arrives with a preconstructed manifest,
one selected node, a trusted `AgentWorkTask`, an objective, model settings,
credentials, and browser context. Qualification adapters supply task-specific
readiness, effect/account, extraction, and acceptance contracts. This is the
correct authority boundary for executing a known task, but it is not yet the
general system that turns a user's goal into editable Work state and approved
execution.

The product still needs:

- a durable `Work` aggregate and versioned domain model for objectives, plans,
  nodes, tasks, resources, actors, questions, approvals, evidence, artifacts,
  and result state;
- general intent clarification and plan authoring without making model text an
  authority or requiring a hand-written Rust task predicate for every goal;
- compilation of one exact approved plan revision into manifests, leases,
  effect/data/account scopes, budgets, expected outputs, and executable nodes;
- a production scheduler that wires the existing orchestration core to actual
  agent/model/browser/service workers and emits stable semantic progress;
- a clean separation between factual/model-mapped completion, native effect
  verification, user acceptance, and durable execution success;
- persistent questions, approvals, unfinished work, results, and safe
  restart/recovery projections without trying to resurrect old execution
  authority;
- provider-neutral planning/execution adapters for local, BYOK, and hosted
  modes, with explicit capability negotiation and no silent degradation;
- product-level context selection, evidence synthesis, memory, and artifact
  publication across more than one browser document or worker;
- typed Rust-to-Svelte projections and intent commands for the Work interface;
- macOS and Windows qualification of the integrated product, concurrent
  Browse/Work behavior, broader authenticated/reversible workflows, and
  measured CPU/RAM/GPU/battery behavior.

The current model catalog/controller path is narrower than the long-term model
surface. OpenAI-backed Luna/Terra are the qualified implementation path;
Anthropic protocol support exists at lower layers, while Gemini,
OpenAI-compatible custom endpoints, local endpoint adapters, hosted routing,
and product model selection still need honest integration and qualification.

The existing screenshot capability is real but intentionally optional. It
should be exposed only for visual/layout evidence that semantics cannot
represent, not used as a default computer-vision loop. Likewise, new tools
should be added because real workflows demonstrate a capability or efficiency
gap—not to maximize the function list.

## 6. The system to build now

The next implementation target is a coherent **Rust Work runtime**, not a
collection of extra browser tools and not the final canvas in isolation.

Conceptually:

```text
User intent and attached context
              |
       persistent Work aggregate
              |
    clarify / understand / draft plan
              |
       editable plan revision
              |
      user approval compiler
              |
 manifest + capability leases + budgets
              |
      Work orchestration runtime
       /       |        |       \
 browser   service   local    specialist
  actor     adapter    tool       agent
       \       |        |       /
        evidence / effects / artifacts
              |
 durable Work state + bounded live projection
              |
        Svelte Work environment
```

### 6.1 Authoritative domain

`WorkId` is the durable, profile-owned identity of one substantial body of
work. It must remain independent of a chat, tab, window, canvas document,
provider session, and execution attempt.

The runtime should distinguish at least these semantic lifetimes:

- **Work:** durable objective, context, structure, results, and unfinished state;
- **plan revision:** editable draft until approved, immutable as execution
  authority after approval, superseded rather than mutated across scope changes;
- **plan node/task:** durable responsibility and dependency state;
- **run/attempt:** bounded execution of one approved revision or node;
- **actor:** human, Zephium agent, sub-agent, specialist agent, or deterministic
  tool with explicit responsibility;
- **resource:** browser context, attached Browse tab, file, service object, or
  other stable input/output identity independent of a run lease;
- **evidence:** immutable provenance-bearing observation/result whose contents
  grant no action authority;
- **artifact:** persistent user-visible output with a typed/versioned payload;
- **question/approval:** durable need for judgment or authority, not a transient
  modal flag;
- **view:** replaceable spatial/layout projection, never the semantic Work.

Persistent identity should use the project-standard durable ID strategy. Native
view handles, callback IDs, and supervisor attempts remain process-local and
must never be serialized as authority.

### 6.2 Intent, planning, and approval

The model may propose clarification questions, a structured draft plan,
dependencies, desired outputs, candidate resources, and estimated requirements.
Rust validates shape, bounds, ownership, provider capability, and allowed
vocabulary. The draft itself grants nothing.

Approval freezes an exact plan revision and compiles it into deterministic
execution authority: identities, profile/account/origin/service scope, data
classes, effects, destinations, budgets, deadlines, required approvals, expected
output schemas, and delegation topology. A material scope change creates an
amendment requiring new approval; it does not widen the live lease.

The current hand-written `AgentWorkTask` approach must evolve without making the
model its own policy or success oracle. General nodes should declare typed
completion contracts: required outputs/evidence, permissible uncertainty,
effect postconditions, and whether completion is mechanically verified,
model-mapped/needs-review, or explicitly user-accepted. Known service workflows
can provide stronger deterministic validators as optimizations. Arbitrary
research conclusions cannot honestly pretend to have the same proof level as a
verified form mutation.

Clarification is part of Work state. If the objective is ambiguous, expensive,
sensitive, or missing a consequential choice, the runtime should create a
question with relevant options/context before authorizing execution. It should
not blindly start because the model can guess.

### 6.3 Orchestration and agents

The first integrated vertical may execute one agent, but the runtime architecture
must preserve the already-defined delegation topology and child-output model.
The primary agent should plan and coordinate; bounded workers execute node-level
responsibilities. A sub-agent receives the minimum node objective, approved
capabilities, relevant context, and output contract—not the entire Work or the
parent's hidden transcript.

Scheduling is a Rust responsibility. It owns concurrency caps, fair admission,
node dependencies, context/resource leases, cancellation trees, backpressure,
deadlines, model/tool budgets, retries, and terminal joins. A model can propose
delegation inside an approved topology but cannot create unbounded workers or
renew budgets. Queued/waiting state must consume no worker or polling loop.

The existing `AgentWorkOrchestration` should be treated as a reusable bounded
core to inspect and either extend or wrap, not as proof that orchestration is
finished. Preserve its move-only execution ownership and refusal behavior. Wire
it to application/runtime workers only through explicit ports and exact
settlement joins.

### 6.4 Capabilities and tool adapters

The browser kernel becomes one executor adapter inside Work. Other adapters may
cover APIs/MCP, files, commands, and specialist agents, but each must expose a
small typed capability contract and independently enforce profile/account/data/
effect scope. Prefer a structured API or MCP route over visual clicking when it
is safer and more efficient. Prefer the browser when it is the available or
product-native route.

Do not expose a generic shell, filesystem, MCP, provider, or JavaScript tool to
every run. Tools default off and appear only through an approved capability
lease. A deterministic Rust operation should replace repeated model-generated
code when a real workflow demonstrates that it improves correctness, latency,
tokens, or safety.

Model input should be a compact projection of the current objective, plan node,
relevant Work state, admitted capabilities, and bounded evidence. Do not replay
the whole Work graph, browser history, or chat transcript on every turn. Keep
page content and service output in explicit untrusted boundaries. Keep
provider-facing schemas phase-specific so cheaper models and practical local
models can use them reliably.

### 6.5 Evidence, results, tasks, and memory

Execution should produce actual typed resources and artifacts, not just final
text. Evidence retains provenance, freshness, sensitivity, omission, and
verification level. Multiple sources may coexist or contradict; collection does
not imply synthesis or truth.

Tasks are durable unfinished Work. Notes/documents are lightweight artifacts
and context; their editor implementation is Tiptap/ProseMirror, but the Work
runtime owns their identity and relationships. User knowledge is deliberately
preserved material; system memory is mostly invisible retrieval context. Agent
output must not automatically become permanent knowledge.

Memory retrieval should filter locally using explicit relationships, recency,
FTS, and bounded ranking before disclosing anything externally. The provider
receives a context manifest explaining what is included and why. Embeddings
remain an optional port until evidence requires a particular implementation.

### 6.6 Persistence and recovery

Extend the normal Store rather than creating a second hidden database or
frontend persistence authority. Persist normalized identity/ownership/status/
ordering/relationship fields and versioned typed payloads. Keep an append-only
semantic audit for consequential transitions, not full event sourcing and not a
dump of chain-of-thought, DOM, token streams, pointer movement, or raw provider
payloads.

Small structured artifacts can build on the existing atomic terminal/artifact
work. Large immutable artifacts should use the planned content-addressed BLAKE3
blob vault with reference accounting and bounded garbage collection. Every
schema ships with migration, rollback/failure behavior, capacity bounds, profile
deletion behavior, and crash tests.

Restart restores durable Work facts, not old authority. It may show that a node
was interrupted, needs review, can be freshly admitted, or has a persisted
result. It must not reconstruct credentials, native refs, a provider
continuation, an execution token, an approval lease, or claim that an uncertain
effect did not happen.

### 6.7 Product projection and frontend boundary

The Rust runtime should expose bounded, versioned projections for Work summary,
plan graph, node/task state, actors, resources, progress, questions, approvals,
artifacts, and terminal/recovery status. Projection revisions and stable IDs
allow Svelte to reconcile without inferring authority.

The frontend sends typed intents such as create Work, edit draft, attach
resource, answer question, approve revision, start/pause/cancel node, request
takeover, or open artifact. It never submits a raw manifest, marks an effect
verified, forges task completion, or persists authoritative Work state itself.

High-frequency canvas movement remains transient UI state and is checkpointed
at bounded semantic moments. Model streams are coalesced before projection.
Large lists and spatial surfaces are virtualized/culled. Live WebViews are
budgeted and shown in dedicated native surfaces; Work cards use semantic state
and bounded recent previews rather than embedding dozens of interactive pages.

The final integration should reuse the separate frontend design system and
units after verifying their actual contracts. Do not throw them away, and do
not bend Rust semantics merely to match a provisional component API.

The frontend foundation is a Svelte 5 projection organized around app,
feature, session, domain, and shared boundaries. Domain stores mirror typed
Rust projections; features own presentation and local interaction state; the
session layer may retain transient frame state; and the privileged frame sends
typed intent rather than mutating product facts. Work is lazy chrome-hosted
presentation over this foundation, while live page interaction remains in
budgeted native browser surfaces. The runtime phase should not reimplement or
reshape that frontend. It should supply the authoritative contracts the
frontend will consume.

Before the streams join, make their thin contract explicit:

- the primary surface distinguishes Browse, an internal page, and a Work
  identified by `WorkId` and a Rust-owned revision; the hosting mechanism is
  not the Work identity;
- Work projections use stable IDs, versioned envelopes, ordered revisions, and
  a bounded full-resynchronization path;
- durable Work semantics remain separate from recoverable, debounced
  `WorkView` state and from disposable gesture state;
- `NoteDocument` is constrained versioned ProseMirror data,
  `ArtifactBlockSpec` is typed agent-visible output, and system memory remains
  private retrieval context; shared rendering does not collapse their storage
  schemas, and a durable Task is not an execution-plan node;
- high-frequency gestures may render an optimistic preview, but native
  geometry is applied by Rust and settled state reconciles to its projection;
- mount/unmount, draft preservation, cleanup, stale revisions, recovery, and
  typed failure states are contract behavior rather than component accidents.

Localization and command presentation remain a shared native/frontend
foundation concern, not Work-runtime semantics. Stable command identities and
one generated catalog should serve both Rust native menus and Svelte; finalized
English labels should not become the IPC contract.

### 6.8 Model modes and hosted seam

Local endpoints, BYOK providers, and the Zephium-hosted gateway are transports
behind one runtime contract. The capability descriptor—not a marketing model
name—determines whether a model can perform a plan/node. Incompatibility is
reported explicitly; there is no silent downgrade.

The browser owns execution and policy in Rust. The initial private hosted
service is a FastAPI gateway for authentication, entitlements, model selection,
bounded relay/streaming, and usage accounting. It is not the Work authority and
does not become required for local/BYOK use. Cloud browsers, remote Work
execution, sync, collaboration, and mobile are separate later systems.

Secrets remain in OS credential storage and out of SQLite/frontend state. The
hosted service stores only the minimum content-free operational/billing metadata
Zephium actually needs; it must not create product telemetry by retaining Work
content.

### 6.9 Reliability, performance, and diagnostics

Every runtime owner needs explicit bounds and terminal behavior: queues,
workers, child agents, browser contexts, model calls, operations, tokens, cost,
payloads, evidence, artifacts, retries, redirects, waits, deadlines, and disk
use. Cancellation and shutdown must drain the original owners or report
recovery debt; replacement empty owners cannot manufacture clean success.

Zero active Work should mean effectively zero agent workers, requests, polling,
model runtime, heavyweight frontend bundle, and idle wakeups. During Work,
measure wall time, CPU time, resident memory, native process/view count,
GPU/compositor pressure, wakeups, disk/network bytes, model tokens/cost, tool
success, human intervention, and Browse responsiveness.

OpenAI Logs with explicit public qualification `store:true` are the source of
truth for what was actually sent to and returned by that provider. Local
content-free diagnostics remain the source of truth for Rust policy, native
dispatch, verification, accounting joins, persistence, timing, and teardown.
Product/BYOK logging stays redacted/stateless by default. Do not add raw page or
model-content logging merely because it is convenient during development.

## 7. Completion boundary for the next phase

The next phase is complete when Zephium can take a normal user objective through
the actual Rust product boundary—not a qualification-specific constructor—and
produce a persistent, inspectable Work with:

- clarified intent or a durable question when consequential context is missing;
- an editable structured plan and exact approved revision;
- one or more executable nodes compiled to bounded authority;
- model-selected tool use through the existing browser actor and at least one
  non-browser adapter where the objective genuinely needs it;
- visible semantic progress and responsibility independent of raw tool logs;
- verified effects, provenance-bearing evidence, and typed artifacts/results;
- pause/cancel/human-takeover and honest uncertain/recovery states;
- restart-safe Work facts without resurrecting old execution authority;
- typed frontend projections ready for the Work interface.

The fastest useful proof is one primary agent completing a real unfamiliar
objective from plain intent to persistent Work state without a scripted route.
Multi-agent execution should then reuse the same plan, node, lease, evidence,
and output contracts rather than introduce a second architecture. Do not spend
weeks polishing multi-agent scheduling before the single primary agent can use
the complete Work runtime; equally, do not design the single-agent vertical so
that adding approved children requires a rewrite.

Acceptance is not one green predicate. Mechanical tests judge contracts,
authority, lifecycle, persistence, and provenance. A person judges whether the
result is factually useful and whether the visible Work state explains what
happened. Repeat real workflows on macOS first, then Windows, with authorized
test accounts and reversible effects. Expand the matrix from observed failure
classes instead of accumulating hundreds of low-information runs.

## 8. Things not to do

- Do not return to polishing one qualification fixture indefinitely now that
  the browser substrate has crossed the handoff threshold.
- Do not claim that the current kernel is already the complete Work runtime or
  release-qualified agentic browser.
- Do not make the Svelte canvas, a transcript, or model output authoritative.
- Do not require a bespoke Rust workflow implementation for every user goal.
- Do not let a model classify its own permissions, sensitive data, effects,
  account, successful mutation, or approval.
- Do not add arbitrary JavaScript/HTML generation, generic native bridges, raw
  selectors, or unrestricted shell/MCP/filesystem access.
- Do not expose all tools on every turn or use screenshots when semantics are
  sufficient.
- Do not confuse structural source mapping with factual entailment or user
  acceptance.
- Do not persist chain-of-thought, provider transcripts, raw pages, or
  high-frequency UI movement as Work history.
- Do not silently resume an interrupted run, retry an uncertain effect, or
  reconstruct authority from persisted descriptive state.
- Do not build cloud execution, collaboration, mobile, teams, Linux parity, or
  capability-pack infrastructure in this phase.
- Do not create a new markdown document for every implementation detail. Update
  this continuation brief when the overall phase boundary moves; update a
  narrow subsystem document only when its durable contract or evidence changes.
- Do not optimize for code volume, tool count, benchmark theater, or ceremonial
  architecture. Optimize for a coherent real product, verified safety, useful
  workflows, responsiveness, and time to an honest integrated result.

## 9. Documentation routing

The repository has many agentic documents because difficult native/security
seams were isolated and recorded while the kernel was being proven. They are
not a linear reading assignment and should not be collapsed into one giant
specification.

Use this routing:

1. Read this document completely for the current checkpoint and target.
2. Read `product-system.md` for the complete product model and release scope.
3. Inspect the actual code and recent commits before deciding implementation.
4. Read `security-model.md`, `architecture.md`, and `frontend.md` only in the
   sections touched by the change.
5. Use `agentic-browsing.md` as the deep browser-actor specification and search
   it for the relevant invariant; do not mechanically restart its old milestone
   sequence.
6. Open a narrow `agent-work-*` document when changing that boundary: execution,
   resources, evidence, results, persistence, artifacts, review, lifecycle,
   forms, inspection, or composition.
7. Use `eval/agentic-browsing/` to understand what was actually qualified and
   what was deliberately not claimed.

Historical detail should remain searchable evidence. If navigation remains
confusing after this map, improve `docs/README.md` or headings/links rather than
deleting records that explain why a safety or lifecycle constraint exists.

## 10. Operating contract for the continuation agent

The main continuation agent is the architect, orchestrator, reviewer, and owner
of the integrated outcome. Use GPT-6 Astra as that main agent. It should decide
architecture itself from product intent, code, evidence, and current research;
this brief is context and constraints, not a demand to reproduce a predetermined
implementation.

The requested collaboration pattern is:

- The GPT-6 Astra high-reasoning main agent owns reasoning, decomposition,
  architecture, tightly coupled vertical implementation, integration, review,
  verification strategy, and final quality. Continuous context is the default;
  delegation is an optimization, not a required ceremony.
- Keep work in the main agent when architecture and implementation are still
  co-evolving, when a task is sequential, or when explaining enough context to
  a delegate would cost more than doing it directly.
- For a substantial bounded implementation or test package whose contract is
  already stable, delegate to GPT-5.6 Sol at high reasoning when this saves
  time. Give it the objective, relevant context, constraints, ownership area,
  and acceptance boundary—not a line-by-line solution.
- For an independent high-judgment implementation or audit that materially
  benefits from parallelism, another GPT-6 Astra agent at medium or high
  reasoning is appropriate. Do not substitute a cheaper model merely because
  the work contains code.
- Use GPT-5.6 Luna only for bounded read-only exploration, repository
  inventory, evidence collection, or other mechanically checkable work that
  can run independently. High reasoning is normally sufficient; max is
  reserved for unusually broad read-only synthesis. Architectural or security
  interpretation stays with the Astra main agent.
- The main agent may make small, obvious, tightly scoped edits itself. Do not
  spawn an agent for a lookup that `rg`, the compiler, or a focused primary
  source answers faster, and do not use an exploration agent for production
  code changes.
- Parallelize only independent work packages. Agents share the worktree, so
  assign non-overlapping files/boundaries or coordinate before edits. The main
  agent reviews every delegated diff and owns integration; a sub-agent's success
  message is not verification.

Engineering behavior:

- Build production code, not a throwaway V0, while still moving quickly toward
  the vertical product outcome.
- Preserve unrelated user changes and reconcile the separate frontend stream
  deliberately.
- Prefer coherent commits containing a contract and its tests or one complete
  vertical behavior. Avoid both tiny noisy commits and multi-subsystem dumps.
- Keep commits local and do not push unless the user explicitly asks.
- Use the smallest relevant test/gate while iterating, then run proportional
  integration, failure, format/lint, and real-workflow verification before a
  handoff.
- Research current or uncertain platform/provider behavior from primary sources
  when it affects architecture. Record a decision only when it remains useful
  after the implementation.
- Do not generate documentation as activity. Update durable docs when the
  product boundary, trust contract, persistence format, or qualification claim
  materially changes.
- Ordinary app launches, isolated-profile rotation, diagnostics, and authorized
  qualification runs do not require repeated user confirmation. Ask the user
  promptly for a real login, Keychain prompt, account, irreversible external
  effect, material product decision, or other authority the agent does not have.

The user is available and can provide disposable Google, Notion, and other test
accounts, approve platform permissions, and perform human-takeover steps. Use
that availability to test real product workflows rather than substituting mock
success, while keeping effects scoped, reversible, and explicit.

## 11. Instruction to the next agent

Continue Zephium from the current repository state by building the full Rust
Work runtime and AI system described here. Begin by verifying the exact checkout
and the separate frontend state that will later be integrated. Understand the
existing browser actor, application admission, orchestration seed, persistence,
and security boundaries before changing them. Then choose and execute the best
production architecture that moves a plain user objective through persistent
Work state, planning and approval, bounded execution, evidence and artifacts,
human control, recovery, and typed frontend projection.

Think independently. If repository evidence or primary research shows a better
mechanism than one suggested here, use it while preserving the product and trust
invariants. Move toward a real integrated workflow quickly; do not spend the
phase collecting hypothetical improvements around an already-proven fixture.
Use the collaboration model in section 10, review all implementation yourself,
leave coherent local commits and evidence, and stop for the user only when a
real decision or external authority is required.
