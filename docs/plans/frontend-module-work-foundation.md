# Frontend module conventions and Work UI foundation

Status: proposed architecture refinement, 2026-09-11. Documentation-only review;
no source moves, runtime contracts, branches or commits are implemented by this document.

The [frontend contract](../frontend.md) describes the current implementation.
[Foundation progress](frontend-architecture-progress.md) records prior verification;
those checks were not rerun for this analysis. This proposal refines the existing
[architecture plan](frontend-architecture.md), particularly module internals, test
placement and the subsequent Work presentation track. On agreement, reconcile the
working contract and tooling together; do not leave conflicting placement rules.

## 1. Current assessment

The first migration established useful inter-feature boundaries, generated-IPC
consumption, public entry points, test infrastructure, native source anchors and
startup-graph checks. It did not finish the frontend architecture or native UX.

Current source inventory illustrates the remaining problem:

| Module | Files directly at its root | Assessment |
| --- | ---: | --- |
| Launcher | 6 | Public API, component, controller, pure model and tests share one level |
| Tabs | 13 | Presentation families, interaction contracts, models and tests need role grouping |
| Settings | 11 (35 total) | Pages/components exist, but model and preview responsibilities remain mixed at the root |
| Sidebar | 3 (7 total) | Header/footer/mode directories encode location rather than a consistent internal convention |
| Domain slices | Usually 2–8 | Focused enough for a small flat source API; separate tests and split by responsibility when needed |
| Session | Flat collection | Transitional navigation, geometry and tool modules still need consolidation and explicit lifetimes |

File count is evidence of growth, not a quality metric. A six-file module can be
clear, while a deeply nested tree can still hide ownership. The missing ingredient
is a predictable internal contract and an explicit rule for how a module grows.

Typed Browse projections, durable preference confirmation, unified surface state,
scoped styles, entry separation and runtime/native qualification remain unfinished.
Temporary lint/dead-code migration exceptions are not final acceptance. Moving files
alone cannot establish performance, accessible interaction or premium visual quality.

## 2. Feature anatomy

Use this vocabulary consistently. Only create a directory when it has real contents.

```text
features/<concept>/
  index.ts                    public components, intents/types and lazy loaders
  components/                 Svelte presentation and interaction components
  model/                      feature-local controllers, selectors, presentation types/state
  tests/                      this module's unit, component and scenario tests
  pages/                      optional: independently navigable destinations
  adapters/                   optional: an actual replaceable external UI-library boundary
```

- `components/` includes the feature's entry component. A component owns its scoped
  styles, semantic markup, focus and pointer/keyboard interactions. Small local runes
  can stay in the component; extraction follows shared ownership or testable behavior.
- `model/` contains feature behavior, not a replica of Rust entities. Keep pure
  transformations in ordinary `.ts`; reactive interaction state uses `.svelte.ts`.
  A search controller belongs here because it coordinates input, cancellation and
  result admission. It is not a generic utility.
- `pages/` is appropriate for Settings destinations. Header and footer are components,
  not pages or independent features. Page navigation metadata belongs in `model/`.
- `adapters/` is earned by a real integration such as a spatial viewport or editor.
  It translates library callbacks and representation data. Native IPC lives below
  the feature, not in a parallel adapter calling Tauri directly.
- Do not add empty `model`, `adapters`, `services`, `hooks`, `utils`, `lib`, `store` or
  `types` folders to satisfy a template. Put a type next to its owning behavior;
  extract a named type module when several implementations genuinely share it.
- Within a feature, pages and components can consume models; models do not import
  components or pages. Adapters depend on explicit representation contracts and do
  not own product semantics. Avoid circular dependencies through the public barrel.
- External consumers use `index.ts`. It is a curated API, not an export of every file.
  Heavy representations expose loader functions; re-exporting a heavy component can
  make an otherwise lightweight public import eagerly load its implementation.

Proposed Launcher organization, using existing code:

```text
features/launcher/
  index.ts
  components/Launcher.svelte
  model/search-controller.ts
  model/search-model.ts
  tests/search-controller.test.ts
  tests/search-model.test.ts
```

Proposed Sidebar organization:

```text
features/sidebar/
  index.ts
  components/
    Sidebar.svelte
    SidebarHeader.svelte
    SidebarFooter.svelte
    SidebarResizeHandle.svelte
    WindowControls.svelte
    ModeSwitch.svelte
  model/                      only sidebar-local interaction, when extracted
  tests/                      only actual behavior tests
```

Keep small component sets together. Add `components/header/` only if the header
becomes a cohesive family with several private parts, not merely because it occupies
the top of the screen. Shared window controls may graduate to shared platform UI
only when real consumers justify that boundary.

### Growth into capabilities

When a feature acquires independently meaningful capabilities, group by capability
before recreating a large horizontal components/model collection. For example,
Launcher may eventually have `search/` and `ask/`, each with the same internal roles.
The launcher entry composes them; a capability does not reach into its sibling's
implementation. Share only genuinely common routing/presentation contracts at the
parent. Do not create those submodules before their behavior exists.

Work's home, clarification, plan, runs, resources, inspection and recovery are such
capabilities. Keep their boundaries explicit in lint as they appear: today's single
`features/work/*` element would otherwise permit arbitrary deep sibling dependencies.
There is no requirement that every capability become a separate top-level feature.

## 3. Tests remain local to ownership, not mixed into source lists

Recommendation: replace file-adjacent tests with `tests/` inside their owning module.
This changes the old plan's mandatory colocation rule; it does not restore one global
`frame/tests` pile.

- `features/launcher/tests/search-controller.test.ts` tests the local controller.
- `features/launcher/tests/Launcher.component.test.ts` runs in the browser project.
- A feature-wide scenario lives in that feature's tests; app composition scenarios
  live in `app/<surface>/tests`. Cross-feature tests do not become production imports.
- Tests may import their own module internals. Cross-module access uses public APIs;
  shared fixtures and mock transport helpers live in `shared/testing`.
- Large capability tests stay in the capability's own `tests/`. Mirror source
  subdirectories only when filenames would otherwise be ambiguous.
- Use explicit `.component.test.ts` for browser tests and `.test.ts` for unit tests.
  Update Vitest discovery, TypeScript inclusion, lint, Knip, Tailwind exclusions and
  production fixture guards in one migration. Do not merely move tests and silently
  stop executing them. Reconcile the old source-sibling discovery convention.

The one-test-file-per-source rule should be a default, not an artificial cap on
independent integration or lifecycle scenarios. Test behavior and contracts rather
than creating empty test counterparts for every file.

## 4. Domain, session and shared have different shapes

A domain slice is already a focused boundary. Start with named source modules plus
`tests/`: `index.ts`, `projection.svelte.ts`, `admission.ts`, `intents.ts` as needed.
Split a large admission/reconciliation model into meaningful submodules; do not force
a component-oriented tree into the domain layer. Existing descriptive names can be
retained where they communicate more than generic `model.ts`.

A frontend projection store is valid. The distinction is authority, not the word
"store": Svelte holds reactive mirrors and temporary state; Rust owns persistence,
execution, permissions, authoritative validation and outcomes.

Group session state by shared lifetime/interaction: `session/surface`, `session/drag`,
`session/motion`, and eventually Work gesture state. Each owns a small API and local
tests. No general mutable session bag. The unified surface model replaces the current
sidebar/tools coordination; reorganizing those files must not preserve the old
coordination under more attractive directory names.

Shared UI stays one kit: one module per primitive, with its component, optional local
behavior/types and `tests/`. `shared/ui/data` contains spec-agnostic data primitives.
`shared/blocks` interprets approved artifact specs without stores or native calls.
Cross-cutting helpers belong in shared only when they have no product opinion.

## 5. Work presentation architecture

The new brief authorizes planning a real Work product surface after the foundation,
using deterministic development scenarios. It expands the old plan's "no Work UI"
scope; it does not authorize inventing the runtime schema. `WorkProjectionV1`,
`WorkCommandV1` and `WorkSignalV1` are conceptual names in the brief. No matching
versioned definitions were found in this checkout's docs, frame or IPC crate.

```text
app/work/                     lazy composition root and host lifecycle
features/work/                Work-only capabilities with components/model/tests
features/<entity>/            reusable task/note/resource presentations when implemented
session/work/                 gesture-only and transient cross-capability interaction
domain/work/                  admitted runtime mirrors and intents
shared/blocks/                approved semantic-artifact renderer
shared/ui/data/               Table, Comparison, Chart, Status and other plain-prop primitives
```

The listing is a responsibility map, not an instruction to create empty directories.
Work capability components use the existing entity public APIs where those entities
exist. They do not clone Tasks or Notes into a separate Work state system. The Work
composition root controls capability composition; Work-to-entity dependencies remain
the explicit one-way exception in the layer graph.

Three boundaries must remain distinct:

1. Runtime state and intents: Rust publishes facts; a domain adapter admits updates
   and sends commands. It never executes a plan in JavaScript.
2. Presentation models: pure selectors map admitted facts to visible rows, cards,
   canvas nodes and inspectors. Formatting and representation choice are frontend
   concerns; execution status and permission availability are not inferred locally.
3. Interaction: focus, selection and gestures remain responsive locally. Meaningful
   edits and consequential actions wait for authoritative settlement.

### Work capability map

These are proposed ownership boundaries, not empty folders to scaffold now.

| Capability | Owns |
| --- | --- |
| Home/objective | New objective entry, completed and unfinished Work navigation |
| Clarification | Questions, options, answer drafts, validation and pending submission |
| Plan | Editable plan representation, dependencies, amendments and selected plan revision |
| Canvas | Viewport, selection/gesture integration, representation placement and spatial navigation |
| Runs | Primary/worker responsibility, semantic progress, blockers and output links |
| Resources | Browser/service/file/tool presentations, preview state and promotion/takeover requests |
| Approvals | Plan and consequential-action approval presentation tied to exact runtime effects/revisions |
| Inspection | Artifact, evidence/source and decision inspectors using shared renderers/entity presentations |
| Recovery | Interrupted/uncertain state, pause/cancel/redirect/resume UX and reconciliation guidance |

The app composition root connects these capabilities. For example, selecting a
resource opens an inspector through a callback/shared selection model; Resources
does not import Inspection's private components. A shared visual status control is
not a second copy of the runtime lifecycle state machine.

### View state and semantic state

Exact live coordinates, pan, zoom and pointer movement are presentation state, not
model-authored execution authority. Recoverable user arrangement is nevertheless
user data: retain the existing Rust `WorkView` boundary for bounded, debounced settled
snapshots scoped by Work and window, separate from semantic audit. Live gestures stay
local. Editor text autosaves separately; leaving Work must not discard user drafts.
A semantic revision should not be advanced by every pan or pointer event. The runtime
contract must specify view-state versioning and conflict behavior.

## 6. Runtime handoff requirements

Do not create a frontend-owned `WorkProjectionV1` and assume Rust will later match it.
Request an owner-approved, versioned schema artifact or generated types before
claiming fixtures are contract-compatible. Until then, plain-prop visual exploration
can proceed, but its types must be labelled presentation-only and not exported as IPC.

| Contract area | Required clarification from the runtime owner |
| --- | --- |
| Identity and scope | Work/profile/window/session ownership, opaque ID rules, revision type and epoch/reset semantics |
| Projection delivery | Full snapshot versus deltas, ordering/admission rules, entity versions, bounded collections, pagination, totals and resynchronization |
| Commands | Payload variants, request/work/revision envelope, admission versus terminal outcomes, correlation, deduplication lifetime and replay scope |
| Conflicts and uncertainty | How stale/conflict/outcome-unknown are reported; how a request is reconciled after disconnect/restart; when retry is safe |
| Questions and plans | Editable fields, validation errors, dependencies, approval revision/effect binding, amendment and superseded-plan behavior |
| Capabilities | Which actions are available or disabled and why; pause/cancel/redirect/takeover/resume transition rules |
| Signals | Work/run/entity scope, sequence/expiry, coalescing, stale-drop policy; signals cannot override durable facts |
| Browser previews | Bounded trusted frame/image reference, generation, expiry, loading/error/redaction, promotion/release/takeover acknowledgements |
| Artifacts and evidence | Versioned variant schemas, provenance, references, uncertainty, mutation operations and unknown-version fallback |
| User data | Note-document schema versus generated artifact schema, draft autosave, WorkView persistence/versioning, recovery on leave |

The command envelope must expose the agreed `request_id`, `work_id`,
`expected_revision` and typed `payload`. Exact variants and response shapes come
from the runtime-owned schema, not this responsibility map.

Treat IDs and revisions as opaque unless the contract explicitly defines ordering.
Do not parse ULIDs or compare arbitrary revision strings lexicographically.
Request IDs can deduplicate delivery; that does not make a purchase or other business
effect inherently idempotent. Do not auto-resubmit an outcome-unknown action. Reconcile
its request identity first. A changed payload/revision must not silently reuse the
same request ID. Accepted admission never becomes a completed-state animation.

The UI should show known committed state, clearly bounded pending edits, conflict
resolution, outcome-unknown and resynchronizing states. An inaccessible or stale Work
is not an empty Work. Unknown artifact versions render an explicit unsupported state,
not guessed content. A partial entity collection cannot produce fabricated totals.

## 7. Artifacts and representative scenarios

Here, product-native artifact components are Zephium-owned Svelte presentations
inside the trusted chrome surface. This does not promise an OS widget for every
chart or table; OS controls, materials and native surfaces remain appropriate at
the established desktop boundaries.

Keep one visual system. Artifact renderers receive validated semantic data plus host
callbacks. Models select supported representations; they never supply privileged
HTML, CSS, Svelte or JavaScript. Shared components own typography, tokens, motion,
focus, accessibility and platform conventions.

A generated document artifact is not automatically a user NoteDocument. Tiptap edits
the constrained note schema; a table/chart/comparison uses its own approved artifact
schema. A checklist item is not a durable Task unless it references a real task ID.
Browser previews and remote images use bounded Rust-authorized references and the
existing decoder/trust constraints; no arbitrary model-provided image URLs or data
URLs are admitted into privileged chrome as a convenience.

Build development scenarios by mounting the actual Work composition and components
through an injected transport port. A dedicated dev-only entry supplies the scenario
adapter. Production uses the live adapter and never imports the scenario registry.
This exercises the intended product UI, not a separate gallery implementation.

Fixtures consume the same approved types, contain deterministic authored transitions,
and have no native side effects, secrets, network calls or persistence. They simulate
known responses, not an independent plan executor or a generic writable Work store.
A scenario should also exercise rejection, stale data and unknown outcomes, not only
an uninterrupted success animation. Validate fixture/schema drift in CI.

Required scenarios:

- Trip planning: clarification, accommodation/transport workers, comparisons,
  provenance and uncertainty, a purchase approval and amendment/conflict paths.
- Product/software research: parallel responsibility, browser sources, comparison,
  decisions, persistent task references and a coding-agent resource representation.
- Interrupted Work: disconnect, uncertain action, recovery, human takeover and
  resumption from newly admitted state rather than replayed frontend assumptions.

The browser resource remains a lightweight preview by default. Activation requests
promotion of the existing Rust-owned context. Render the promoted/owned state only
when acknowledged; release returns it to preview through that same context lifecycle.
A canvas card does not allocate a live WebView.

## 8. Quality and delivery

For each module, record responsibility, public API, state lifetime, async cleanup,
loading/failure/recovery states, test entry points and performance-sensitive paths.
A short module-level comment or README is sufficient; complex Work capabilities merit
a README. Do not create documentation boilerplate for every component. A concise
future `frame/AGENTS.md` should route agents to the authoritative contract and gates,
not repeat or diverge from it.

Performance acceptance measures cold Browse, first Work open, large Work updates,
repeated enter/leave and hidden-idle behavior. Preserve entity identity across unrelated
updates, subscribe per visible concern, separate progress signals from semantic data,
and avoid reconstructing the whole canvas for a worker-progress change. Virtualize
measured large collections. Bound decoded preview memory, active preview count,
subscriptions and background work; cancel/release them on leave. Do not invent budget
numbers without measurements or claim unmount unloads imported JavaScript modules.

The visual foundation needs defined control states, typography/density, surface roles,
focus treatment, motion interruption, reduced motion, text expansion, forced colors,
keyboard traversal and native macOS/Windows behavior. Token extraction alone does not
establish premium UX. Validate actual Browse and Work compositions, including empty,
error and interrupted states. Native materials remain native; heavy editors/charts
load per representation and hidden Work has no recurring feature activity.

Delivery sequence:

1. Agree these internal conventions and reconcile docs/tooling; migrate existing
   roles without implementing missing product features or changing Rust semantics.
2. Finish Browse contracts, surface/lifecycle coordination, styling ownership,
   entry/graph budgets and foundation verification from the existing plan.
3. Obtain the versioned Work handoff; document exact missing semantics with its owner.
4. Build Work UI vertical slices against approved types and deterministic scenarios,
   using the real composition, renderer and intent paths.
5. Integrate live runtime transport; verify persistence, real effects, recovery,
   authority, platform behavior and resource use end to end.

Independent frontend work can continue while unrelated runtime CI defects are owned
by their track. Record baseline failures separately; do not weaken those gates or
claim full qualification until resolved. Avoid repeating the earlier interpretation
that one unrelated full-CI failure prevents all independent foundation progress.

Keep this analysis and existing working tree uncommitted. Before implementation
handoff, use a short-lived frontend integration branch with coherent changes and
cross-boundary Rust/generated types in the same commit when required. Coordinate the
branch base with the runtime owner; do not race a shared branch or blindly apply a
large patch over their changes. Branch creation is not accomplished by documentation,
and the current dirty work must not be lost or committed wholesale to create it.
