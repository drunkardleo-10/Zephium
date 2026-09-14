# Zephium Work runtime continuation brief

Status: integrated implementation handoff; historical runtime evidence retained

Latest product checkpoint: 2026-09-13

Current branch: `work-mode-integration`

Integrated baseline: `c5a7b08e` (`Integrate Work runtime and frontend foundation`)

## Current integration checkpoint — read first

**2026-09-14 M1 checkpoint (branch `work-mode-integration`, commits through
`22e82359`).** The Work shell was rebuilt: full-bleed chrome on the native window
material, vertical icon tools with Tabs centred, a docked composer, a tasks
capsule, per-kind cards, a FLIP lift editor, an inspector, two-finger pan and
pinch zoom, in-place remote view reconcile, and Areas as parent nodes with
persisted placements (`WorkEnvironmentView.areas`). Semantics were extended
additively at artifact `version` 1 under the profile schema gate (now 21):
`ComparisonMatrix`, `Findings`, structured `EvidenceCollection` entries, chart
measurement bases, claim-level evidence indices, `Subject`/`Finding` environment
references, explicit relations, and the `Object` resource kind with
Rust-resolved `PreserveArtifact`. The synthesis schema offers the new kinds.
The frontend renders matrices, findings, and titled sources, shows an agent
avatar during execution, and organises newly published results around the
objective. Native qualification of this checkpoint is pending user review;
the isolated app builds from `desktop/tauri.work-integration.conf.json`.
Remaining milestones: in-Work browser pane (M2), context admission, typed
documents, media, account-scoped work (M3), qualification (M4).

**2026-09-14 M2 checkpoint (commits `c04c3a4d`, `df0c299e`).** Pages open
inside Work in a transient floating browser pane. Rust owns the native hole:
`shell/work_pane.rs` keeps one Space tab (existing or freshly navigated) in a
clamped, session-only rect above full-window chrome, `visible_tree()` replaces
the split tree in every lifecycle path so the pane leaf is discard-protected and
recreated after a crash, modal prompts remove the page while the pane persists,
and return, page switch, tab close, and scope changes clear it.
`LayoutState.work_pane` carries the applied rect, presentation, and generation;
chrome reports its measured hole through `work_pane_set_rect` (coalesced, stale
generations ignored). Pane dismissal keys are menu bindings enabled only while a
pane is shown; `Cmd+W` hides the pane inside Work. The frontend pane renders the
presentation sentinels for its tab, follows the applied rect (echoes are
distinguished from native clamps), and reports opening, failed, and unavailable
states honestly. Native qualification of Escape-from-page, focus after dismiss,
and geometry after resize/reopen is pending user review.

**2026-09-14 M3 checkpoint (commits `e7977c44` … `eddf03dc`).** Context
admission: `zephium-app/src/work_context.rs` resolves the user's selected
canvas elements to bodies at operation begin (notes, tasks, objects, tab title
and URL, objectives, results, subjects, findings), truncates and digests them,
and binds `WorkContextDisclosureV1` to the plan revision
(`WorkEdit::ReplaceDraftDisclosed`) or execution spec
(`WorkRequest::RuntimeCommandDisclosed` for public reads). A changed body
refuses with `Conflict` (Stale); private context on a public read refuses with
`ReviewRequired` and chrome routes through the reviewed plan. Public search
never receives bodies inside a reviewed plan; synthesis does, after
re-admission verifies every digest. `work_context_preview` returns the same
manifest for the composer chip. Decisions: `WorkEnvironmentEdit::Decide` /
`Undecide` record the user's choice per element; planning admission appends
current decisions as implicit private items. Documents: synthesis emits typed
blocks compiled by `zephium-core/src/work/document.rs` into the note schema
plus derived paragraphs; prose links must match a disclosed evidence URL; notes
carry a `link` mark opened through native intents.

**2026-09-14 M3 media checkpoint (commits `0dbf57f2` … frame `admit candidates`).**
`ResourceContent::Media { asset: MediaAssetV1 }` describes bytes in a
profile-scoped, content-addressed store (`<data>/media/<profile>/<digest>`,
`zephium-store/src/hub/media.rs`). Admission sniffs bytes, decodes images with
the `image` crate under fixed limits (8 MiB imported, 2 MiB fetched, 8192 px),
recognizes PDFs, and keeps other files opaque; the store mints the resource in
the same call and IPC callers cannot forge a Media draft. Images reach main
chrome only through `zephium-media://localhost/<profile>/<digest>` (CSP
`img-src` widened accordingly); other kinds open with the OS default app.
Subjects carry `image_candidates`; `media_admit_remote` fetches one candidate
without cookies (`zephium-agentic/src/public_asset.rs`, public HTTPS only,
three redirects, 2 MiB), stores it with `Fetched` provenance, and relates
subject → media with `Uses`. Live file references (`Reference{scoped path}`)
and in-Work PDF viewing are deferred; PDFs open with the OS.

**2026-09-14 M3 account checkpoint.** Account-scoped work runs on the
existing owned-context runtime, not on a tab lease: the user picks an attached
tab ("Ask signed in"), chrome sends `WorkOperationV1::PrepareAccount`, and
`zephium-app/src/work_account_scope.rs` resolves the tab, mints a single-step
plan when the Work has none, mints an opaque `AgentAccountId`, and returns an
approval draft whose node capability is `WorkCapability::AccountRead { scope }`
or `AccountUpdate { scope, update }` (`WorkAccountScope` names tab, page URL,
origin, account). Approval is the user's attestation of the account (the review
requires an explicit checkbox); Zephium has no independent account collector
and says so. Execution (`zephium-work-composition/src/account_scope.rs`) opens
the page in a Work-owned page that shares the profile's cookie store
(`isolated_public` false), with a `UserAttestedAccount` source for reads and a
`FieldUpdateTask` for updates: exactly two verified fill transitions on one
uniquely matched public text control (optional accessible name), each from a
fresh observation, then an extraction bound to the restored value. Interventions
persist on the execution (`WorkExecutionFact.intervention`): the adapter maps
`NeedsHuman` / `ModelRequestedHuman` to closed kinds (sign-in, challenge,
permission, unsupported interaction, review), and a user takeover cancels with
`WorkRuntimeIntent::Cancel { intervention: HumanTakeover }` before the pane
shows the tab. Continuation is a fresh approval of the same spec ("Run again");
no pause/resume is promised. Borrowed-tab leases, the human sign-in handoff
context kind, and independent account collectors remain unbuilt; the retained
Notion qualifiers stay feature-gated and are superseded by the product path.

[`product-system.md`](product-system.md) now records the agreed full product
direction: Profile → Space → Work → Area, objective-independent manual Works,
the canvas as the primary environment, a centered Tabs toolbar popover opening
on first entry, a single Work profile menu, a bottom composer, and a compact
task control. Browse's sidebar is hidden in Work. AI and Work can be disabled
independently. Context starts from a bounded semantic view of the current Work;
provider-native search and Rig reuse fit behind Zephium's authority boundaries.
Read that document before interpreting the earlier runtime notes below.

The runtime and frontend histories are already integrated. Preserve both,
`backup/runtime-before-work-ui-integration`,
`backup/frontend-main-before-work-ui-integration`, and unrelated work. Do not
reset, rebase, separate, replace, or push the integration branch. The current
integration changes are uncommitted; inspect `git status` and the actual diff
before editing or making coherent local commits.

The integrated implementation now has objective-independent Rust Work environments,
Space membership, Areas, explicit browser/resource/objective/artifact references,
and separate presentation revisions. Existing `WorkId` remains an objective and
execution identity; `WorkEnvironmentId` is the persistent canvas. No execution
identity is rewritten. Schema 19 introduced environments; schema 20 adds a bounded
revision-scoped checkpoint replay window. Only semantic edits consume immutable
command receipts. Old checkpoint identities remain stale after receipt eviction.
Profile scrubbing includes all environment tables.

The native Work host now renders the live canvas with Browse chrome hidden,
create/reopen, the Tabs popover, Notes attachment/editing, objective controls,
exact approval/start/cancel, historical results and independent AI/Work settings.
Generated IPC and explicit projection adapters connect these to Rust. Rig maps
provider requests behind Zephium's exact-byte admission and transport; it does
not own scheduling or authority. Provider-native search is now a bounded approved runtime capability, with an
exact provider/model/query grant, original-attempt settlement, durable provider
citations, and no native browser allocation. The preferred qualified search
profile is Luna; legacy mini approvals remain readable without hidden fallback.
Semantic answer rejection settles known usage when the provider envelope proves
completion; uncertain dispatch or accounting remains unknown. Canvas attachments
are not yet admitted model context. Browser activation currently returns to the
exact retained Browse tab; a dedicated Work pane/takeover remains incomplete.

Native qualification on 2026-09-13 uses the isolated
`app.zephium.work-integration` application and Node 24.19.0. Canvas/objective
reopening and the schema 19→20 migration were observed. Genuine execution
`01M2DNMRVGR7B8C2YRT4HSDWGT` reached `cancel_requested` then durable `cancelled`;
uncertain child usage remains a conservative reservation. A normal restart did
not restore worker authority. Abrupt-restart execution
`01M2E0F790QQM1PQSCHZDMPPEX` was terminated after its original public-search
attempt had been running for five seconds. Reopening projected interruption from
the stale owner without redispatch. Explicit acknowledgement then persisted
`interrupted` with attempt `01M2E0H26E314NK63GSQG4SP85` left `outcome_unknown`,
usage unknown, and zero artifacts. Read-only observation deliberately does not
rewrite the original running fact; acknowledgement is a durable user command.
Completed-result reopening is recorded below.

Public qualification has advanced but is not complete. Single-objective execution
`01M2DX1GB3HXYZPBHFJRRK1SFD` produced a durable evidence collection identifying
Svelte issue #18096, with four citations, explicit caveats, and mitigation advice.
The original issue was independently checked as open. The native search took
13,119 ms, used 13,982 model tokens, conservatively accounted 14,498 micro-USD,
and allocated zero browser resources. Provider trace
`resp_06d1f0566235ad69006aa6dd69b20087d292821d1849219a94` includes the opaque Work,
execution and attempt metadata. The user judged the information provisionally
useful, pending deeper review, but rejected the presentation as substantially
below the intended product quality. Two earlier
mini searches lacked usable citations; their failure/unknown facts remain intact.

The real four-responsibility comparison execution
`01M2DXE06214DS594DR6MQKNBC` persisted its first successful Svelte Flow research
branch (15,857 tokens, 30,141 ms, eleven citations), then the coordinator failed
before the remaining branches. This is partial evidence, not a completed
multi-agent result. The handoff failed because it copied an entire research
artifact into the smaller dependency envelope. The fix projects bounded,
explicitly truncated dependency summaries and citation passages while preserving
original Store publications, source keys and all existing admission limits. A
three-child regression with realistic answer sizes and 33 citations passes;
The next native run, `01M2DXX1AT4DQFSVH4BK9NDHWC`, completed all three
research branches (47,556 tokens in total; 49,958 micro-USD conservative
accounting) but its primary failed before publishing synthesis. All three
originals and their 37 citations remain durable. A subsequent run exposed the
exact synthesis refusal: 8,529 counted input tokens exceeded the planner's 8,192
allowance. Synthesis now has a separate 32,768 input-token ceiling; its existing
32KiB disclosure, request-size bounds, and original attempt budget still apply.
The native rerun `01M2DZS3SMRZH3PBZP67B84ZGS` completed three research branches
and published a typed Comparison, reaching durable `needs_review`. It took
126,545 ms, used 58,971 tokens across four attempts, conservatively accounted
55,772 micro-USD, and allocated no native browser resources. The parent received
three compact outcomes with 48 source references and published 46 evidence
references. This proves native multi-agent publication, not human-rated utility:
the table is too verbose, contains raw Markdown/local citation indices, and
needs review for category distinctions and unsupported platform assumptions.
Saved originals remain unchanged. Keychain wait timing is now separately logged
from provider execution. Source navigation
also exposed an OpenUrl/native-return sequencing bug and incorrect frontend
handling of deferred outcomes; targeted Rust and browser regressions pass.
The rebuilt native app now opens the real GitHub source in Browse. The completed
single-objective artifact, its explicit canvas attachment and original citations
also reopened after process restart without restoring worker authority.

Results are projected directly onto the canvas from durable execution facts;
explicit attachment saves a same-profile artifact reference independently of
layout. Automatic projection prioritizes exact root outputs; child and overflow
results remain accessible through the historical execution review. Focus result
preserves placement while restoring readable zoom. Embedded tables retain one
visible title and minimum column widths. Native vertical reading was verified;
real-trackpad horizontal scrolling and overall presentation acceptance remain
pending. Stale-owner recovery opens the exact interruption review, suppressing
misleading Start/Cancel controls until the appropriate durable transition.

An explicit Research public web workflow is now implemented in source through
generated IPC. It atomically admits an internal one-node plan and execution for
the exact submitted objective, provider/model and bounded allowance, without
separate planning/approval screens. Its provenance is user-directed public
reading, not reviewed-plan approval. Only the original fresh Store callback can
dispatch; replay, reopening and ordinary Start cannot restart an admitted read.
It excludes attached/private/account context and rejects overlong queries rather
than shortening them. Store rollback/replay/scope tests, application dispatch/
cancel/reopen tests and strict desktop Clippy pass. This shortcut has not yet
been qualified with a live model through the native UI. Desktop operation replay
still resolves credentials before receipt reconciliation; unavailable credentials
can prevent that operation replay, although projection and cancellation remain
available. The ordinary complex-work entry and intended visual planning
experience remain unfinished. Recent checks include frontend unit and WebKit
tests, provider transport tests, and actual-capacity/reopen settlement regressions. They do not substitute for useful
multi-agent output, takeover, and
authenticated read/write qualification. Development logs retain bounded identities,
counters and closed failure enums, never prompts, page contents or credentials.

Sections 3–8 below retain earlier implementation evidence and runtime design
context at their named baselines. Their older sequencing, separate-stream
assumptions, gap lists, and green-build statements are historical, not current
instructions or qualification claims. Current code/tests establish implemented
behavior; the revised product document establishes direction; security-model
establishes enforced guarantees.

### Earlier runtime handoff baseline

Browser foundation baseline: `c88e6d1` (`Close bounded navigation grounding gaps`)

Durable runtime baseline: `68d28d51`; subsequent fixes and live evidence are in section 3.3.

Prepared: 2026-09-11

Earlier checkpoint: the first macOS read-only Work flow was ready for frontend
integration. Versioned authoring/query/planning/review interfaces, durable
authoring replay, immutable originals with user edits/acceptance, bounded current
answers in assignments, public approval preparation and reusable execution
dispatch now exist. Generated types: `crates/zephium-ipc/bindings/work-v1.ts`.
The product qualifier `--live-public-work-product` passed original browser/child
and primary execution, explicit test review/edit commands, stale refusal,
idempotent replay, clean shutdown and Store reopen
(`/tmp/zephium-product-native-live.log`,
`target/work-runtime-proof/product-integration.json`). Its research content was
honestly inconclusive; this is lifecycle proof, not factual certification.
Consequential actions stay unavailable, and planning generations remain
non-replayable rather than pretending to have a durable call ledger. Those
limits supersede the older blanket integration gates below; finish further
product behavior with the frontend connected instead of indefinitely extending
the isolated runtime.

[`work-frontend-context.md`](work-frontend-context.md) preserves the earlier
frontend handoff. It does not describe a separate stream that still needs merging.

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
- [`frontend.md`](frontend.md) controls the checked-in Svelte frame. The design
  system and runtime are already integrated; inspect their actual contracts.
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
- Work can exist without an objective or AI; substantial delegated work uses
  relevant context, clarification when needed, a visible approach, exact
  approval, execution, and persistent results;
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

## 2. Integrated delivery sequence

Follow product-system section 14. The foundation histories have met; the next
unit of delivery is a real user behavior across persistence, Rust authority,
generated IPC, native hosting, and Svelte presentation. Stabilize shared
contracts before parallel implementation, then integrate small coherent vertical
behaviors continuously. Neither a detached backend subsystem nor a fixture-only
canvas completes this phase.

Keep the primary agent responsible for the tightly coupled domain and native
integration. Delegate substantial independent work only after its boundaries
are clear, with explicit ownership and review. Model choice and parallelism
are implementation-session decisions, not properties of Zephium's product agent
topology. The user is reviewing the next implementation allocation separately
from this product-direction update.

## 3. Historical runtime implementation checkpoints

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

The handoff baseline's Work persistence was narrower than the product requires. It proved
durable admission/terminal CAS behavior, recovery classification, audit
delivery, and optional atomic storage of a bounded source-mapped extraction. It
did **not** persist the complete Work, plan, task/resource graph, questions,
approvals, view-independent outcomes, or orchestration state. The extensions
below close some of those gaps; the complete runtime remains the target.

### Product authoring slice after the handoff baseline

The continuation implementation adds a Rust-only durable authoring path through
`Handle::work_document`, the normal Store actor, and profile schema 15. Core now
owns the same canonical `WorkId` re-exported by agentic resources. Plain intent,
clarification questions/answers, bounded editable plan revisions, semantic audit,
CAS, history, and restart-safe full projections persist independently of runs.
The follow-up adds active/archive/delete lifecycle, explicit history compaction,
application-owned identity minting through `WorkIntent`, temporary-key draft
proposals, clarification supersession and dismissal, and author provenance.
See the product-authoring section of `agent-work-persistence.md` for the contract,
limits, migration decision, and tests.

The optional `zephium-app/work-planning` service now connects an explicit
profile/Work/revision to one model-backed clarification or draft and one Store
CAS. The OpenAI adapter uses a separate bounded objective/context disclosure,
shared fixed-endpoint transport, provider input counting, catalog pricing, and
strict proposal validation. No browser observation, manifest, or approval is
manufactured to admit planning. See the planning section of
`agent-work-persistence.md` for the contract and qualification boundary.

Planning usage is returned to the caller, including final
CAS failures, but a durable planning-attempt/accounting ledger and restart
reconciliation are still needed before autonomous scheduling or product-wide
budget enforcement. `plan_ready` remains descriptive and cannot authorize
execution. Dropped/uncertain generation is never automatically retried.

The checked-in frame is the earlier browser frontend, not the separate Work
design-system stream. No frontend implementation is part of this phase.

### Durable execution and browser adapter extension

Profile migration 16 adds execution facts and idempotency receipts on the same
profile connection and Work revision sequence as authoring. Approval binds an
exact immutable plan revision plus explicit typed scopes and per-node/global
budgets. One active execution is allowed per Work; approved plans survive
compaction. User command replay returns its original receipt plus current facts,
never another execution. Active execution blocks authoring changes and deletion;
explicit cancellation is distinct from worker settlement. Dropped attempts retain
unknown usage rather than receiving a zero-cost refund. The Store incarnation
and its original monotonic epoch govern deadlines; persisted wall timestamps
cannot renew them, and a new incarnation cannot resume an old attempt.

`zephium-app/work-runtime` adds dormant `WorkRuntimeService` and move-only
`WorkNodeAttempt` ownership. Original Begin acknowledgement is required before a
worker is exposed. Host-only settlement stays pinned to its admitted regular
profile even when window focus changes; user projections still check the current
surface. Store independently checks attempt identity, dependencies, concurrency,
budget, output contract and revision. Definitive CAS conflicts can retry only
settlement; an uncertain callback never reruns the adapter. A lightweight observer
retains one replaceable `WorkSignalV1`, with no worker, timer or native authority.
An attempt also receives at most 6 KiB of typed artifacts from its approved direct
dependencies in the same execution. Sibling outputs and provider transcripts are
excluded. The browser treats these as untrusted research context and must source
its own result from original page evidence.

`zephium-work-composition/durable-runtime` consumes that live attempt into the
existing model-directed Public browser controller. Scope and budgets compile
through the original manifest/navigation admission; there is no scripted route,
selector, generic JavaScript or shell port. The original Work identity reaches
the resource owner. Persistent extraction is read through its original handle,
source IDs are resolved in that exact archive, and explicit native resource
closure must be acknowledged before semantic result publication. Terminal task
state alone is insufficient for closure. Browser accounting currently retains
the approved conservative ceiling, explicitly labelled, rather than claiming
exact totals from transient progress events.

Core provides bounded `WorkArtifactV1` data for documents, tables, comparisons,
charts, checklists, evidence collections and browser previews. Presentation hints
are separate and contain no canvas geometry or executable markup. The current
browser adapter produces source-mapped documents requiring review; the other
kinds have bounded synthesis support; only the explicitly recorded live artifact
kinds are qualified. IPC
exports `WorkProjectionV1`, `WorkCommandV1`, `WorkSignalV1`, Specta types and JSON
fixtures. The execution command vocabulary currently covers approval,
cancellation and interruption acknowledgement. Authoring still uses its existing
Rust intent API; no complete frontend command bridge is claimed.
`Handle::work_evidence` resolves a citation on demand only after verifying its
membership in that Work and profile. The original extraction's digest and source
identity are checked before returning a bounded historical quote. This read does
not claim the execution fence or restore any native/action authority.

The direct primary/child extension now joins a fresh `Coordinate` primary to the
existing `AgentWorkOrchestration` owner. Only this owner can admit a direct child
through the original parent attempt. A private move-only `WorkNodeSettlement`
joins the child's original Store acknowledgement to the supervisor's one-shot
recipient delivery. Loading projections cannot recreate it. Combined dependency
and parent-completion cycles, widening scopes/budgets, premature parent success,
foreign receipts and duplicate child dispatch are refused. Unknown child outcomes
retain their reservation; missing children cannot trigger primary model work.
Cancellation permits the original adapter's bounded cleanup without renewing its
execution deadline. The first application driver supports one primary and direct
children in host-selected approved order; deeper trees fail closed.

`WorkSynthesisDisclosure` selects bounded typed dependency artifacts and historical
quotes with local evidence keys. The fixed-endpoint OpenAI adapter shares the
planner's exact input counting, admission, budget, cancellation and unknown-outcome
transport ownership. Rust resolves returned local citations to original evidence,
enforces output contracts and persists usage before presenting review artifacts.
The primary uses this adapter only after its children return. Structured output
uses an internal adjacent `kind`/`value` encoding so the discriminator precedes
its content; durable/IPC artifacts retain their existing flat data representation.
Normal requests use `store: false`; public qualification can explicitly opt into
storage through `probe-harness`, which optimized builds reject.

The real standalone synthesis qualification passed on 2026-09-11: five actionable,
incomplete release-checklist items, 959 reported model tokens, conservative price
accounting, `NeedsReview`, and identical artifact data after Store reopen. The
report is `target/work-runtime-proof/synthesis.json`; the passing log is
`/tmp/zephium-synthesis-live-qualified.log`. Earlier flat-schema responses were
valid documents but failed the requested checklist quality assertion; these were
not counted as successful qualifications. This run qualifies checklist synthesis,
not every artifact kind or primary/child native execution.

Remaining integration includes dynamic primary decisions, compact selected
resources/decisions, deeper and parallel workers, consequential-effect approval
and settlement, user result acceptance/editing, durable planning/model-call
accounting and the complete frontend transport. Standalone Begin still refuses
children: a persisted edge never substitutes for original live parent admission.
Do not present this slice as the complete runtime or frontend-ready delivery.

The opt-in `macos-terra-agentic-probe --live-public-durable-work` entry (feature
`durable-runtime`) exercises one requested Public SQLite-documentation note from
a real Luna plan, through explicit bounded qualification approval and native
browsing, to artifact closure, Store reopen and historical citation reads. The
minimal native application host dispatches ordinary Shell bootstrap and forwards
native policy events; it does not substitute its own ready profile policy. It
presents a foreground qualification window and pumps ordinary AppKit lifecycle
events. Shipping observation ownership checks still apply; changing the focused
application can legitimately refuse observation. The qualifier accepts
up to four model-proposed responsibilities and runs their approved dependency
order sequentially, dividing its fixed reservation between them. This loop is
qualification dispatch, not the production primary/child scheduler.
It uses the fixed development
Keychain credential, at most $0.10 planning reservation and $0.50 browser
reservation. A successful run writes a public semantic report under
`target/work-runtime-proof/`; the existence of the runner alone is not evidence
that it passed. Real parent/child and consequential-action qualification remain
required.

On 2026-09-11 this native durable workflow passed with a real Luna plan and
model-selected public route: one source-mapped note, acknowledged original
resource closure, Store reopen, identical durable projection, and successful
historical source reads. The local report is
`target/work-runtime-proof/public-research.json`; the run log is
`/tmp/zephium-durable-funded-live.log`. Its status is `NeedsReview`, not factual
certification or user acceptance. The saved note explains the WAL shared-memory
limitation and its stored evidence includes the matching official SQLite passage.
This qualifies the public browser adapter vertical only; it is not parent/child
execution or consequential-effect approval evidence.

Earlier native failures are not all attributable to focus changes. An unchanged
scope reproduced `RequestPolicy(Budget)` after eleven decisions under the old
100,000-token/$0.20 approval. The successful qualification uses a fixed
400,000-token/$0.50 ceiling, 24 model calls and the same 120-second execution
deadline. Policy and request-contract refusals now preserve their closed cause
instead of collapsing into generic `Authority`. No budget is automatically
renewed. The budget-denied native run also ended in retained recovery; that
failure-path cleanup still needs qualification. The adapter allows at most 30
seconds from a close request for cleanup and never reports unknown closure as
success. Public provider trace storage is an explicit qualification-only opt-in;
normal product calls remain stateless.

The coordinated qualifier (`--live-public-coordinated-work`, same probe feature)
adds a real model-authored two-node plan, original primary and native child,
structured publication handoff, and primary synthesis. Its first live run on
2026-09-11 did not pass: the worker alternated between SQLite's documentation
index and list, then proposed another visit to the list after the visit ceiling.
The original policy refused `Browser(Navigation(Navigation))`. Native recovery
and Shell shutdown did not settle cleanly; no primary artifact or reopen success
was claimed. The failure log is `/tmp/zephium-coordinated-native-live.log`.
The stored public input confirms four completed hops and repeated list visits;
this failure is not evidence of a focus problem. The qualifier now finishes the
original poisoned coordinator and records `coordinated-attempt.json` before
shutdown, retaining unknown child accounting and avoiding a primary model call.
That attempt report is diagnostic state, not resource-closure or reopen proof.
A second fresh run (`/tmp/zephium-coordinated-native-second.log`) reached a native
`Accounting` refusal and again failed to drain shutdown. Its attempt report
verifies the corrected failure settlement: primary `Failed` with exact zero usage
(no synthesis dispatched), child `OutcomeUnknown` with missing usage and retained
reservation, execution `Interrupted`, no artifacts.

The follow-up investigation found a concrete accounting defect: the receipt
array supports production discovery, but `AgentRunAccountingSnapshot::navigations`
still counted only its first two entries. A third committed hop made terminal
metric closure refuse `Invariant`, including during otherwise settled failure
cleanup. The regression reproduced the old result of two after three real
policy-accounted navigations. The fixed reducer counts the whole bounded array;
tests cover successful closure after three/six hops and refusal after four.
Closed accounting causes now survive controller diagnostics, including the first
journal refusal. Navigation, visit, token, deadline and resource limits were not
widened.

Native failure and cancellation now have independent qualification reports.
`/tmp/zephium-coordinated-fixed-qualified.log` records a refused, unobserved search
URL, original resource/Shell closure and identical failed facts after Store
reopen (`target/work-runtime-proof/coordinated-failure.json`). The new
`--live-public-cancelled-work` mode submits the normal durable cancellation
command after native model admission, continues polling the original adapter,
and requires acknowledged child cancellation, no primary synthesis or artifacts,
clean resource/Shell shutdown and identical reopened facts. It passed in
`/tmp/zephium-coordinated-cancelled-live.log`; its report is
`target/work-runtime-proof/coordinated-cancelled.json`. The stopped primary has
exact zero usage; the child retains its explicit conservative reservation.

Stored public traces also showed that the model treated an initial viewport
capture of a long document list as a whole-document inventory. Browser
instructions now explain initial capture coverage and expansion through a
current observed container; no site-specific route is supplied. The next native
run successfully produced the cited WAL findings, then exposed a separate
primary-synthesis admission defect: the owned runtime request lane omitted
`ReadEvidence`. That read now uses the original pinned profile and existing
Store checks for artifact membership in the exact Work. Private-profile and
unlinked-evidence refusal remain enforced. The partial run is recorded in
`/tmp/zephium-coordinated-coverage-live.log`; it is not a successful combined
qualification.

The corrected combined workflow passed on 2026-09-11 in
`/tmp/zephium-coordinated-evidence-qualified.log`. A fresh model-authored plan
created the original primary and browser child; the child chose its route and
published cited WAL findings, and the primary consumed those findings and
published its own explanation. Both attempts succeeded, both artifacts require
source-mapped review, native resource/Shell shutdown completed, and the identical
projection and historical citations reopened from Store. The report is
`target/work-runtime-proof/coordinated-research.json`, Work
`01M291H9MYJTTPGW56P6ZX11QD`. The primary used 1,276 tokens and a 521-microUSD
conservative charge; browser accounting still explicitly reserves the approved
ceiling rather than pretending to have exact aggregate usage.

The stored primary response is
`https://platform.openai.com/logs/resp_0c14813b09248e6b006aa460a0f16087d2a26389b5f2409e7b`.
Its retrieved input contains one compact responsibility/output contract, one
child document and two historical evidence previews with local keys. It contains
no child browsing transcript, durable authority IDs or live resource handles.
This proves the original direct-child publication-to-synthesis join; dispatch
order and topology are still host-selected from the approved plan, not a dynamic
primary scheduling policy. Consequential approval and complete product authoring
remain separate gates before full frontend integration.

The follow-up regression checkpoint passed 619 core tests, 75 controller tests,
and 12 application Work tests (two explicit live qualifications ignored),
including the pinned evidence-read boundary and original coordinator joins.
Strict Clippy passed for core/controller/application libraries and tests, and
workspace formatting and diff checks passed. Logs:
`/tmp/zephium-runtime-closure-final-tests.log`,
`/tmp/zephium-controller-closure-final-tests.log`,
`/tmp/zephium-work-app-final-tests.log`, and
`/tmp/zephium-runtime-closure-final-clippy.log`.

The IPC fixture set now also contains the live research, cancelled and failed
projections plus the research evidence previews under
`crates/zephium-ipc/fixtures/work-{research,cancelled,failed}-v1.json` and
`work-research-evidence-v1.json`. They preserve original terminal facts, usage
qualifiers and citation relationships without serializing execution authority.
All three Work IPC tests and strict IPC Clippy passed
(`/tmp/zephium-work-live-fixtures-tests.log` and
`/tmp/zephium-work-live-fixtures-clippy.log`). They validate the fixtures against
Rust domain invariants; this is not a complete fixture set for every artifact
kind or recovery state.

The extension's deterministic regression checkpoint passed 1,149 tests across
application, core, IPC, Store and composition (four explicit qualifications
ignored), plus nine shared planning/synthesis provider boundary tests. This
includes the `durable-runtime` composition feature. Logs:
`/tmp/zephium-runtime-final-regressions.log` and
`/tmp/zephium-provider-final-regressions.log`. Strict Clippy passed for those
runtime libraries/tests plus the provider with `-D warnings`
(`/tmp/zephium-work-runtime-clippy-final.log`); the native qualifier builds.
An additional all-targets probe lint run encountered four pre-existing diagnostic
probe lints in the native accessibility/responder/trusted-edit/owned-surface
fixtures; that broader graph is not claimed lint-clean.

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

## 4. Historical evidence at the named backend checkpoint

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

At that historical checkpoint, focused checks passed: 20 provider-request tests and 45 controller
tests, plus a successful debug application bundle. These checks are a narrow
regression statement, not a replacement for the repository's broader gates.

## 5. Historical gap analysis (recheck against the integrated tree)

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

The implemented authoring, durable execution, direct-child and synthesis slices
are described in section 3. The remaining product scope includes:

- extending the durable Work model with selected resources/decisions,
  consequential approvals and user result acceptance;
- general intent clarification and plan authoring without making model text an
  authority or requiring a hand-written Rust task predicate for every goal;
- compilation of one exact approved plan revision into manifests, leases,
  effect/data/account scopes, budgets, expected outputs, and executable nodes;
- extending the direct-child orchestration adapter with dynamic primary decisions,
  nested/parallel workers and complete product progress delivery;
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

## 6. Runtime boundary guidance from the earlier phase

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

## 7. Earlier runtime completion boundary

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
- Preserve unrelated user changes and the already-integrated frontend/runtime
  histories. Do not repeat or undo their integration.
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

Continue from `work-mode-integration` and its actual uncommitted state. Read
the current checkpoint above and product-system before the historical runtime
guidance. Preserve the integrated histories. Understand the browser actor,
application admission, persistence, generated contracts, native host, and real
frontend components before changing them. Implement the agreed shared human
and agent environment, including manual Work, actual tab continuity, contextual
execution, persistent useful objects, intervention, and recovery. Do not turn
the runtime graph into a card transcript and call it the product.

Think independently. If repository evidence or primary research shows a better
mechanism than one suggested here, use it while preserving the product and trust
invariants. Move toward a real integrated workflow quickly; do not spend the
phase collecting hypothetical improvements around an already-proven fixture.
Use the collaboration model in section 10, review all implementation yourself,
leave coherent local commits and evidence, and stop for the user only when a
real decision or external authority is required.
