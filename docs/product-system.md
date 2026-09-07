# Zephium product system

Status: product and target-architecture source of truth
Last reviewed: 2026-08-31

This document defines what Zephium is, what the first public release must be,
and the architectural decisions that let the product grow without being
rewritten. Product direction is fixed at this level; individual interaction
details and replaceable implementation choices can evolve when evidence says
they should.

This is not a claim that every described capability exists in the current
tree. `architecture.md` remains the source of truth for the implemented browser
foundation, `security-model.md` for enforced security guarantees, and
`agentic-browsing.md` for the first implementation program behind Work.

## 0. Program context

Zephium is not beginning as an AI mockup around a generic browser shell. Its
WebView-based browser foundation has already validated the hardest architectural
risk: native page lifecycle, low application overhead, native Rust blocking,
and substantial Chrome-extension compatibility. Active extension work has
validated difficult extensions including Vimium, Dark Reader, and 1Password on
macOS and is bringing Windows to parity. This is engineering-program context,
not a shipping claim; the current tree and `security-model.md` determine what
is enabled and guaranteed at any exact revision.

Extension work may continue concurrently with Work foundations. Changes to
profile construction, native extension controllers, page principals, view
lifecycle, or platform forks must therefore be coordinated rather than
silently rebased over either subsystem's invariants.

The project also starts with meaningful distribution: the creator's Terax
open-source audience and launch experience have produced nearly 10,000 GitHub
stars and substantial product interest. Zephium should be engineered for
thousands of real users from its first release, not as a disposable prototype
that will be rebuilt if a launch video succeeds.

## 1. Product thesis

Zephium has two native ways of using the web:

**Browse** is a premium, lightweight, private browser. It must be worth using
when every AI capability is disabled.

**Work** is an environment for accomplishing complex work through the web and
the rest of the computer with humans, agents, tools, knowledge, and persistent
state.

The central insight is:

> Complex work should not disappear behind a chat interface. It should have
> visible, persistent, directly manipulable state.

Language is often the fastest way to express intent. The environment is a
better way to represent complex state. The agent is an actor in that
environment, not the interface itself.

Zephium is therefore not positioned as a canvas browser, graph browser,
productivity browser, research browser, AI browser, agentic browser, task
manager, or browser with built-in tools. Those are components or properties.
The product identity is **Browse + Work**.

## 2. Product principles

The following are release requirements, not aspirations:

1. **No novelty tax for browsing.** Opening GitHub, watching a video, reading,
   searching, and managing tabs remain ordinary, excellent browser actions.
2. **Browse stands alone.** AI and Work can be disabled completely without
   degrading the browser.
3. **Work state is visible.** Objectives, plans, evidence, execution,
   responsibility, questions, approvals, artifacts, and unfinished work are
   inspectable while they evolve.
4. **State survives the transcript.** Useful resources, tasks, decisions,
   knowledge, and artifacts persist after a conversation ends.
5. **Human control is structural.** A user can inspect, redirect, pause, take
   over, approve, reject, or manually complete work. Control is not a panic
   button added after autonomous execution.
6. **Native components before generated webpages.** Agents populate bounded,
   accessible Zephium components with structured data. They do not inject
   arbitrary HTML into a privileged surface.
7. **Local-first and privacy-conscious.** Local state remains local by default.
   Only the minimum relevant context crosses a model or service boundary.
8. **One product architecture.** Local models, BYOK, and Zephium-hosted models
   use the same work, agent, tool, and result contracts. Provider choice must
   not create different products.
9. **No silent degradation.** If a model, platform, or integration cannot meet
   a capability contract, Zephium explains the incompatibility and asks for a
   compatible choice. It does not quietly produce an inferior workflow.
10. **Best-in-class engineering is part of the product.** Correctness,
    responsiveness, CPU, RAM, GPU, battery, security, recoverability, and
    debuggability are acceptance criteria for every subsystem.

## 3. Browse

Browse owns the familiar web experience:

- windows, profiles, spaces/workspaces, tabs, splits, and navigation;
- horizontal, vertical, compact, and power-user browsing arrangements;
- platform-native presentation and integration;
- native Rust ad and tracker blocking;
- practical compatibility with major Chrome extensions;
- privacy-first defaults, zero application telemetry, and open-source code;
- customization through validated product concepts rather than injected code;
- optional notes, tasks, history, analytics, focus tools, command/search, and
  contextual AI.

Platform WebViews provide rendering. Zephium owns browser state, lifecycle,
resource policy, interface, product features, and the integration model around
them. Being WebView-based is an engineering constraint, not the product
identity.

### 3.1 Browser interface

The normal browsing experience is centered on a polished sidebar. It has a
full state for tabs, spaces, profiles, and browser controls, and a compact state
that leaves room for one active native utility such as Notes, Tasks, History,
Analytics, or contextual AI. Utilities should appear when useful and collapse
cleanly when not.

A global Raycast/Spotlight-style command surface can expose browser search,
commands, notes, tasks, actions, and later Work. It remains one native product
surface rather than a parallel application.

### 3.2 AI in Browse

Browse may offer page-scoped help such as summarization, translation,
extraction, questions about the current page, and small actions. It is compact
and contextual, not Zephium's defining AI interface. A conventional chat may
exist where it is the simplest representation, but chat is never allowed to
become the primary product shell.

If AI is disabled, Work is hidden for the current product model. Notes, tasks,
history, analytics, and other non-AI utilities remain independently usable and
independently disableable.

## 4. Work

Work is a distinct full-window environment for substantial goals: development,
research, product work, operations, management, recruiting, design, content,
learning, purchasing, decisions, administration, and other multi-step work.
The web is its natural foundation, not its boundary. Work may also use files,
commands, APIs, MCP servers, connected services, and specialist agents such as
Codex or Claude Code when the user authorizes them.

Work is not a larger AI sidebar, a tab overview, an execution log, or an
infinite whiteboard full of webpage screenshots. It is the shared working
surface on which the actual state of an objective exists.

### 4.1 Core loop

Substantial work follows this conceptual loop:

1. **Clarify.** Zephium determines the real objective, constraints, missing
   choices, accounts, scope, and expected outcome. Questions can contain
   interactive options and evidence rather than only prose.
2. **Understand.** Relevant resources, previous work, knowledge, services, and
   constraints are collected into visible state.
3. **Plan.** The plan becomes editable product state with dependencies,
   responsibilities, effects, costs, and approval boundaries.
4. **Approve.** The user approves an explicit scoped plan rather than a vague
   autonomous request.
5. **Execute visibly.** Agents and tools operate while their purpose, inputs,
   progress, questions, and outputs remain inspectable.
6. **Persist the result.** Resources, artifacts, decisions, tasks, and useful
   knowledge survive. A transcript is supporting evidence, not the product
   result.

Understanding and execution are separable. A user can ask Zephium to build a
complete model and recommendation, then execute all, some, or none of it.

### 4.2 Work home

Entering Work first presents a calm home rather than an empty canvas or chat:

- recent and active Works;
- unfinished tasks and items needing attention;
- a compact intent composer;
- a contextual shelf of current or recently relevant browser resources.

The shelf suggests context. Attaching a tab creates a relationship to a Work;
it does not move the tab out of Browse.

### 4.3 Work identity

A `Work` is a profile-owned, persistent top-level aggregate. It is not a tab,
window, note, chat, canvas document, or browser session. It contains or
references objectives, plan state, tasks, resources, actors, artifacts,
questions, approvals, knowledge, and views.

Tabs and browser contexts can be attached to Work, but tab identity and Work
identity remain independent. Cross-profile copying is always explicit; a Work
never silently crosses a profile privacy boundary.

### 4.4 Spatial environment

A spatial surface is the strongest current representation for Work because it
can show structure, parallelism, dependencies, and heterogeneous artifacts at
once. It supports pan, zoom, selection, move, resize, grouping, meaningful
connections, focus, and collapse.

It is semantic, not a general drawing application. Freehand whiteboarding and
arbitrary decorative layout are not initial goals. Automatic layout is valid
only when it carries meaning and preserves user placement; the product must not
shuffle state merely to look dynamic.

Information chooses the best native representation: cards, tables, maps,
timelines, documents, diagrams, source structures, code, tasks, images,
comparisons, and browser resources can coexist. A graph is one representation
inside the environment, not the product model.

### 4.5 Browser resources in Work

Tabs are one kind of resource, not the organizing unit of Work. Keeping many
interactive WebViews embedded across a zoomable canvas would create poor
interaction, focus, accessibility, compositing, and resource behavior.

The default browser-resource representation is a compact semantic card with
favicon, title, purpose/state, origin, and a recent bounded frame or preview
when useful. Selecting it promotes the exact browser context into a dedicated
native page surface or non-overlapping split where user and agent can operate.
Returning to the spatial view refreshes its latest preview. Visibility is a
presentation state, not a transfer between unrelated browser systems.

Work-owned contexts, borrowed tabs, human takeover, and exact lifecycle rules
are specified in `agentic-browsing.md`.

### 4.6 Native visual primitives

Zephium owns a versioned registry of structured visual primitives. The initial
vocabulary is expected to cover:

- Card and resource collection;
- Browser resource and source;
- Image and media;
- Map;
- Table and comparison;
- Chart;
- Timeline;
- Task and checklist;
- Note and document;
- Code and diff;
- Agent and tool activity;
- Artifact;
- Diagram and graph;
- Status, question, and approval.

Each primitive has a typed, bounded, versioned specification, stable identity,
accessibility behavior, interaction contract, renderer, persistence codec, and
migration path. Models can request a supported component and populate its
data. Rust validates admission. Unknown versions fail safely or render a
bounded fallback. Generative UI can be explored later through equally bounded
contracts; arbitrary agent-generated HTML or script is never the default.

### 4.7 Actors and delegation

A Work may contain the user, a primary Zephium agent, bounded sub-agents,
specialist external agents, deterministic workflows, local tools, connected
services, and local models. Responsibility must be visible at the part of the
work it affects.

External agents complement Zephium. A coding branch can be handed to Codex or
Claude Code while its objective, context, progress, review, and result remain
part of the parent Work. Integration is through capability-scoped adapters,
not screen scraping when a structured CLI, API, or MCP route exists.

### 4.8 Plans, effects, and approvals

An approved plan is a scoped execution lease. Actions explicitly represented
by that plan may execute without repetitive confirmation. The lease binds the
actor, profiles/accounts, origins/services, data classes, allowed effects,
destinations, budgets, and expiry or run lifetime.

Zephium must stop and amend the plan before crossing a new scope, account,
origin, destination, data-transfer boundary, cost, or material effect. Account
creation, purchases, messages, publishing, deletion, destructive changes, and
other irreversible effects must be visible plan nodes rather than hidden tool
steps.

Two-factor prompts, CAPTCHAs, security challenges, and other actions that
cannot be delegated safely become `NeedsHuman` state. They are not disguised
as ordinary approval prompts. A human can take control and the agent resumes
from newly observed reality.

## 5. Tasks, notes, knowledge, memory, history, and analytics

These are native projections of browsing and Work, not independent products
competing with Todoist, Notion, Obsidian, or RescueTime.

### 5.1 Tasks

Tasks represent unfinished work. A task can survive its Work session, appear
in Browse, reopen exact context, retain related resources, be completed by a
human, and—within an approved capability—be delegated. A checkbox rendered in
an AI answer is not a task unless it has a durable task identity.

### 5.2 Notes and documents

Notes are lightweight user artifacts and context. Rich editing uses Tiptap
directly with a deliberately limited schema and versioned ProseMirror JSON.
Only the active document mounts an editor; read-only and spatial projections
use lightweight renderers. Collaboration packages and heavy extensions are
not loaded until a real product requirement exists. Notes are a well-built
unit in the system, not a second product or a reason to build an editor engine.

### 5.3 Memory and knowledge

Memory has two conceptual layers:

- **System memory** is mostly invisible, structured context such as history,
  relationships, previous decisions, tasks, site context, and interaction
  patterns used for retrieval.
- **User knowledge** is information deliberately worth revisiting: decisions,
  findings, explanations, references, notes, documents, and other artifacts.

Agent output does not automatically become permanent user knowledge. Zephium
can suggest preserving useful material. Retrieval filters locally first and
sends only the relevant bounded context to an external model. The user can
inspect a context manifest showing what will leave the device and why.

### 5.4 History and analytics

History can grow beyond URL, title, and timestamp into contextual browsing and
Work history. Analytics can explain what was worked on, which resources
contributed, what remains unfinished, and where time went. Processing is local
where practical, opt-in where sensitive, and never a surveillance or
advertising channel.

## 6. AI and agent architecture

Zephium has one agent architecture with replaceable model transports:

```text
Intent / Work state
        |
Rust agent supervisor and policy engine
        |
Typed model, browser, tool, memory, artifact, and approval ports
        |
Local runtime | BYOK provider | Zephium hosted gateway
```

The Rust process owns run state, planning contracts, tool scheduling,
capability enforcement, cancellation, budgets, browser execution, durable
semantic commits, and recovery. A model proposes structured operations but is
not security authority. Page content, model output, extension output, MCP
results, and remote service output remain untrusted input.

### 6.1 Model modes

The initial model surface supports:

- OpenAI, Anthropic, and Gemini BYOK adapters;
- OpenAI-compatible custom endpoints;
- local runtimes such as Ollama, LM Studio, and llama.cpp endpoints without
  bundling an inference runtime initially;
- a Zephium-hosted model gateway.

All implement the same typed streaming and tool-use contracts. Capability
descriptors express structured output, tool calling, images, context limits,
and other requirements. Work chooses only eligible models and explains why a
requested model cannot run a workflow. Provider changes never bypass an
approval or capability boundary.

Secrets live in the operating-system credential vault and are zeroized from
temporary memory where practical. API keys, refresh tokens, and desktop
service credentials never live in ordinary SQLite fields or frontend state.

### 6.2 Hosted gateway

Hosted AI is part of the first product, but hosted browser execution,
multiplayer, mobile continuation, and the larger cloud ecosystem are not.
Initially the gateway authenticates, validates entitlements and budgets,
selects an eligible provider/model, relays bounded requests, streams outputs,
and accounts for provider-reported usage.

The hosted gateway is a separate private Python service using FastAPI. Its
authentication, entitlement, billing, persistence, cache and deployment
adapters remain behind explicit service boundaries; their concrete products
must be selected from operational evidence rather than becoming browser-domain
dependencies. The public browser does not require an account when hosted
features are unused, and it must not initialize hosted connections in that
state.

The gateway stores no prompts, page contents, model outputs, screenshots, or
tool payloads for product analytics. Operational and billing records are the
minimum content-free metadata required for abuse controls, reliability, and
accounting. Downstream model providers retain their own disclosed policies;
Zephium must not claim that BYOK or hosted relay changes those policies.

## 7. Domain and persistence model

Authoritative product state remains in Rust under the existing functional-core,
imperative-shell architecture. Svelte is a projection that sends typed intent.
Side effects flow through explicit ports and adapters.

The target domain adds, without collapsing existing browser concepts:

- `WorkId`: durable profile-owned ULID;
- `ActorId`, `RunId`, `PlanId`, and `TaskId`: durable ULIDs where identity
  survives a process;
- `ResourceId` and `ArtifactId`: durable identities for attached state;
- `ContextId`: browser automation identity independent of tab identity;
- runtime-only native view handles: compact process-local integers;
- `WorkView`: replaceable presentation state separate from Work semantics.

Persistence uses normalized envelopes for stable identity, ownership, status,
timestamps, ordering, and relationships plus typed, versioned payloads for
component-specific data. Current state is accompanied by an append-only
semantic audit of meaningful execution and approval transitions. This is not
full event sourcing and must not become a dump of model reasoning, DOM data,
or high-frequency UI movement.

Large immutable artifacts use a content-addressed BLAKE3 blob vault with
reference accounting, bounded metadata, atomic writes, and garbage collection.
Search begins with SQLite FTS5, recency, explicit relationships, and graph
signals. An `EmbeddingPort` remains optional; prefiltered bounded exact cosine
search is sufficient before a measured need for a managed vector service or
immature embedded ANN database.

The current local database relies on operating-system file protection and does
not claim application-level encryption. Persistence contracts reserve an
encryption migration seam without inventing a false guarantee today.

## 8. Frontend and component stack

The fixed foundation remains Svelte 5, strict TypeScript, Vite, Tailwind CSS v4,
narrow Bits UI adoption, Hugeicons, Geist, generated typed IPC, and native OS
surfaces where they fit. `frontend.md` is the working frontend contract.

The following additions are preferred because each remains replaceable behind
a Zephium-owned domain boundary:

- **Tiptap/ProseMirror:** limited note/document editing schema;
- **`@xyflow/svelte`:** only as a viewport and interaction adapter if a
  representative benchmark proves acceptable; it never owns Work semantics,
  persistence, or layout decisions;
- **Pointer Events and Svelte actions:** spatial movement and resize; native
  drag-and-drop only at OS or WebView boundaries, with no generic DnD framework
  by default;
- **ECharts:** tree-shaken behind a bounded `ChartSpec`; SVG for many small
  charts and Canvas for large datasets as measured;
- **TanStack Table/Virtual:** large tables and lists;
- **MapLibre:** lazy-loaded only when a map is present;
- **CodeMirror 6:** lazy-loaded only for editable code surfaces;
- **Paraglide:** compile-time typed localization.

Markdown is parsed and sanitized into typed blocks at a trusted boundary. Raw
`{@html}`, arbitrary CSS, model-produced JavaScript, and executable theme
payloads are prohibited.

### 8.1 Performance rules

The frontend pays for capabilities only when visible:

- Work, editor, map, chart, code, and spatial libraries are lazy-loaded;
- one active rich editor exists unless a measured use case proves otherwise;
- long lists and large canvases are spatially culled and virtualized;
- live native browser contexts are explicitly budgeted, not created per card;
- animation is limited to compositor-friendly properties and respects reduced
  motion;
- model streams are coalesced before projection rather than causing a render
  per token;
- spatial movement is transient UI state and persists at bounded semantic
  checkpoints, not every pointer event.

Zero AI/Work activity must mean effectively zero agent runtime, network,
polling, model, and heavyweight frontend overhead.

## 9. Design system and native quality

Zephium should feel like a premium operating-system application: quiet,
legible, responsive, and obvious despite the complexity underneath. The aim is
Apple-level quality, not a platform-neutral imitation of macOS. Windows follows
Windows interaction conventions with the same restraint and finish.

The design system owns semantic tokens, density, typography, motion,
accessibility, control states, layout primitives, visual components, platform
adaptation, and documented composition patterns. User customization changes
typed validated tokens and product options. It never injects arbitrary CSS or
script.

On macOS, the complete native Tao window uses the best supported native window
material: Liquid Glass on macOS 26 and later when the pinned native stack can
prove it, with vibrancy as the fallback. Svelte must not recreate system glass
using `backdrop-filter` or expensive CSS blur. On Windows, prefer proven Mica
and native materials with a solid Windows 10 fallback; Acrylic is retained only
where resize, GPU, power, and contrast measurements justify it. Menus, sheets,
and cross-WebView surfaces remain native according to `architecture.md`.

A specialist design agent may later own tokens, components, interaction polish,
visual QA, and accessibility evidence. It does not change Rust authority, IPC,
security boundaries, domain semantics, native layout invariants, or persistence
without an explicit architecture review.

Localization is structured from the beginning, including pluralization,
formatting, pseudo-locale, text expansion, and RTL validation. English may be
the only launch translation. AI-assisted translations can be reviewed later;
model output never bypasses the localization catalog and QA process.

## 10. Security and privacy

The enforced browser trust model in `security-model.md` remains non-negotiable.
Work adds more untrusted data and therefore must narrow, not weaken, those
boundaries.

Core rules include:

- the Rust shell is the only product authority;
- privileged application WebViews are local, navigation-locked projections;
- raw pages, agent pages, extensions, models, remote services, and connected
  tools receive no generic native bridge;
- model output is a proposal decoded into bounded typed values;
- page and service content is always labelled hostile data, never instruction;
- source-to-sink policy checks data sensitivity, origin, destination, account,
  and effect before execution;
- secrets are never included in page snapshots, prompts, logs, artifacts, or
  model-visible fields;
- plan approval is revalidated at the actual effect boundary;
- cancellation and user takeover revoke active automation authority;
- diagnostic traces default to semantic metadata and require explicit local
  opt-in for sensitive payload capture;
- there is no application-owned product telemetry.

MCP, CLI, filesystem, terminal, and external-agent adapters default off. A user
enables them through profile-scoped capability leases. Hosted entitlements do
not silently expose local tools or data.

## 11. Reliability, performance, and observability

Every production subsystem defines before implementation:

- ownership and lifetime;
- bounded memory, concurrency, queues, payloads, retries, and timeouts;
- cancellation and shutdown behavior;
- crash and partial-failure recovery;
- durability boundary and migration plan;
- trust classification and authorization point;
- performance and resource measurements;
- deterministic tests and representative integration/evaluation evidence.

Performance budgets are derived from a committed baseline on supported
hardware, then enforced as reviewed deltas. Measures include startup and input
latency, wall-clock task latency, CPU time, resident memory, native view/process
count, GPU/compositor pressure, wakeups, energy impact, disk and network bytes,
and battery behavior. A feature that is fast in a synthetic microbenchmark but
causes idle wakeups or multiplies native WebViews is not optimized.

Runtime behavior uses bounded workers, backpressure, cancellation tokens,
generation-aware results, typed failures, idempotent or explicitly
non-idempotent effects, and no blind retry. User-visible success means the
effect was verified, not merely requested.

## 12. First public release

The first release is a complete product rather than a demo or deliberately
crippled MVP. It includes:

- a polished production Browse experience on macOS and Windows;
- native blocking and practical major-extension compatibility;
- Work with clarify/plan/approve/visible execution/persistent result;
- a high-quality useful set of native visual components;
- production agentic browsing;
- tasks, notes, user knowledge, and memory integrated with Work;
- local models, BYOK, and hosted AI through one architecture;
- Browse-side contextual AI;
- the security, recovery, resource, and quality evidence needed for real users.

Linux remains in the architecture and build discipline, with full product
parity expected after the first release. Major Chrome extensions are the
compatibility target; Zephium does not claim the entire Chrome extension API.

The following are deliberately post-release but must attach through existing
ports and identities rather than require a product rewrite:

- cloud browser and Work execution;
- mobile progress, approval, and continuation;
- real-time multiplayer collaboration;
- teams and organizations;
- domain capability packs;
- remote long-running workers and broader cloud synchronization.

## 13. Business and openness

The browser and capable local Work remain open source. Free/local value can
include Browse, blocking, extensions, customization, local Work, local models,
BYOK, notes/tasks/memory, local integrations, and external-agent adapters.

Paid value corresponds to real hosted value: Zephium-hosted AI, compute,
premium models, cloud browser execution, long-running and parallel hosted
agents, synchronization, continuation, collaboration, and organizational
capabilities. The browser is not artificially impaired to manufacture a
subscription.

The hosted service may remain private and live in a separate repository. Open
browser protocols and data formats should still make local use, export, and
practical self-hosting possible where doing so does not falsely promise a fully
supported hosted product.

## 14. Delivery architecture

Implementation proceeds through production vertical slices that leave evidence
behind:

1. Prove agentic browsing independently of Work UI, following
   `agentic-browsing.md`.
2. Introduce Work domain identities, persistence, audit, and projection ports.
3. Build the model/runtime supervisor and hosted/BYOK/local adapters against
   the same contracts.
4. Build the spatial shell and initial native component registry.
5. Connect clarify, plan, approval, execution, browser resources, and
   persistent outcomes end to end.
6. Add integrated task, note, knowledge, memory, and Browse contextual
   projections.
7. Complete the release matrix, recovery, security, accessibility, resource,
   and performance gates on macOS and Windows.

This order proves the core risk first. It does not authorize temporary
architecture, insecure bridges, throwaway persistence, or prototype-quality
code. Each slice must have explicit bounds, typed contracts, migrations where
needed, tests, metrics, and reviewable commits.

Commits should represent coherent production changes: a contract and its
tests, a platform adapter and its gates, a vertical behavior and its evidence.
Avoid both line-by-line noise and one or two unreviewable commits containing an
entire subsystem. Generated files and migrations travel with the change that
requires them.

## 15. Decision discipline

Zephium does not need an ADR ceremony for ordinary implementation. This
document and the subsystem specifications hold accepted decisions. A short ADR
is justified only when a choice is expensive to reverse or changes one of:

- trust, secrets, permissions, or data boundaries;
- durable identity, persistence, sync, or migration shape;
- cross-platform native architecture;
- public extension, tool, model, or cloud protocols;
- ownership of authoritative state;
- a release guarantee or irreversible dependency.

An ADR records context, decision, alternatives, evidence, consequences, and a
revisit trigger. It is not a status report and does not substitute for code,
tests, benchmarks, or this product source of truth.

## 16. Non-goals and anti-patterns

Do not:

- force Work, AI, notes, tasks, or analytics into normal browsing;
- turn Work into chat with a decorative graph;
- treat tabs as the Work data model;
- place many full interactive WebViews across a canvas;
- expose model chain-of-thought, raw DOM, CDP logs, selectors, or tool noise as
  product state;
- generate arbitrary privileged HTML, CSS, or JavaScript;
- make notes/tasks/analytics independent application clones;
- send broad history or personal context to a model because it may be useful;
- let provider choice bypass capability or security requirements;
- promise seamlessness by silently degrading behavior;
- auto-arrange spatial state without semantic purpose;
- add polling, background workers, or heavyweight bundles to Browse when a
  capability is unused;
- confuse an API request with a verified real-world effect;
- use cloud dependence to weaken the open local product.

## 17. Success

Browse succeeds when people switch because Zephium is an exceptional browser,
even if they never enable AI.

Work succeeds when a person can express a substantial goal, understand the
resulting plan, watch humans and agents construct useful state, intervene
without losing continuity, and leave with real resources, decisions, tasks,
knowledge, and artifacts—not merely a transcript.

The combined product succeeds when that power feels calm and native while its
resource use, security, stability, and engineering quality remain worthy of a
browser used by thousands and eventually millions of people.
