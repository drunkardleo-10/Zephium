# Frontend foundation handoff

Current implementation checkpoint: 2026-09-12.

The frontend foundation is available for Work runtime integration. This is a
source and capability handoff, not a claim that the Work product is implemented or
that the repository is release-qualified. [frontend.md](frontend.md) is the
permanent architecture and contributor reference; this document records the
current delivered state and its limits.

## Architecture and ownership

The frame uses Svelte 5 runes, strict TypeScript, Vite 8, semantic tokens and
Zephium-owned controls. Bits UI supplies selected accessible behavior. The native
browser and utility panel have independent HTML entry points and startup graphs.
Rust owns durable resources, identities, permissions, native geometry and legal
transitions. Svelte owns projections, drafts, selection and gestures.

| Location under `frame/src/` | Current responsibility |
| --- | --- |
| `app/` | Native-surface composition and feature assembly |
| `features/` | Product presentation, local interaction and lazy surface loaders |
| `session/` | Shared transient state within a WebView document |
| `domain/` | Rust projections, intent admission and reconciliation |
| `shared/` | UI primitives, typed IPC, lifecycle helpers and isolated test utilities |
| `styles/` | Tokens, visual axes and remaining global product styles |

Features use a curated `index.ts`, `components/`, `lib/` where supporting behavior
exists, and `tests/`. Domain slices retain focused source files and tests. There
are no mandatory empty role folders. Cross-module imports use public APIs; internal
imports are relative. Dependencies flow downward. The utility host has an explicit
entity-composition exception; entity features do not depend on Work. Architecture,
module roles, types, styles and unused code are checked by the frame toolchain.

The normal IPC binding is `frame/src/shared/ipc/bindings.ts`, generated from Rust,
including Work operations. `crates/zephium-ipc/bindings/work-v1.ts` remains a generated
runtime reference; the frontend does not import it or maintain a second wire schema.

## Native browser and Work host

The sidebar Browse/Work switch requests `browser.work` or `browser.return` through
the existing command transport. Rust's `BrowserPage::Work` borrows the established
internal-surface layout. Native page WebViews are suppressed while the internal
surface is active. Returning to Browse uses the existing synchronous chrome
restoration and exact tab-identity checks. The mode control follows native
projection, not command admission alone.

`app/browser/WorkWorkspace.svelte` now mounts lazy Work home/detail views over the
profile-bound Rust services: creation/reopening, clarification, editable plans,
model planning, historical execution/plan joins and seven semantic artifact kinds.
`domain/work` retains exact pending commands and revision-bound drafts across mode
changes. Rust owns bounded keyed operations independently of UI observation and
joins workers before Shell/Store shutdown. Notes and Tasks remain independent.

The standalone Work demo, preview HTML entry and separate preview build are
removed. Trip, research and interruption scenarios exist only under Work tests;
production graphs reject fixtures and test modules.

## Reusable presentation components

| Module | Available capability |
| --- | --- |
| `features/work` | Lazy canvas and host-controlled Work presentation surface |
| `shared/ui/data/Artifact` | Inert document, table, comparison, chart, checklist, source and browser-resource representations |
| `shared/ui/data/Chart` | Lazy LayerChart SVG bars with original-decimal table fallback |
| `shared/ui/data/DataTable` | Lightweight accessible paginated table |
| `shared/ui/data/DocumentEditor` | Lazy, constrained paragraph/text Tiptap draft editor |
| `shared/ui/data/Evidence` | Bounded historical excerpts, truncation and original byte counts |
| `features/notes` | Rich note editor and resource list, available through its lazy loader |
| `features/tasks` | Task editor, completion, due dates and resource list, available through its lazy loader |

Svelte XYFlow provides canvas interaction. Inputs are bounded to 500 items and
2,000 relationships. Identity and arrangement survive unrelated data updates;
invalid saved coordinates/zoom are rejected. Nodes do not create semantic
connections or delete entities. View callbacks expose coordinates and viewport,
not execution truth. Culling begins at 100 items; XYFlow still initially measures
nodes. Keyboard inspection and the Work presentation surface's list alternative
are available to a host with real data.

`WorkSurfaceView` and its intent/request types are presentation values, not copies
of Work IPC. The reusable surface includes objective and clarification controls,
artifact inspection, explicit action confirmation, and pending/rejected/conflict/
unknown/resynchronizing states. The native Work host now mounts these components.
Action meaning, exact scope and request outcomes come from its host.
A fulfilled callback or animation does not establish durable completion.

The surface releases callback observations and unmounts heavy children when
inactive. Its local authoring and arrangement state is not a durable Work draft
store. There are no worker pools, model routes or execution lifecycles in this UI.

Artifacts render semantic data; they never execute agent-generated HTML, CSS,
Svelte or JavaScript. Browser-resource cards currently describe resources; they
have no live WebViews, safe-frame capabilities or promotion/takeover authority.
Chart support is bounded SVG bars, not a general chart suite. TanStack Table is
not a dependency. The artifact document editor is intentionally narrower than
the independent rich Notes editor.

## Notes and Tasks resources

Authoritative resource types live in `crates/zephium-core/src/resources.rs`.
Persistence lives in `crates/zephium-store/src/hub/resources.rs`, through the Store
actor. `domain/resources` supplies profile-bound projections and transient drafts.
Native IPC supports bounded lists, reads, note-reference resolution and idempotent,
revision-checked mutations. Identity, revisions and saved state come from Rust.

Notes contain validated Tiptap data, formatting and same-profile note references.
Tasks contain title, description, completion, due date and pin state. Search,
pinning and soft trash/restore are implemented. Notes are deliberate user knowledge
resources, not agent-system memory. Tasks are user resources, not Work plan nodes
or execution attempts. Agent-facing resource authorization is not implemented here.

Resource drafts autosave after one second of inactivity. Writes are single-flight;
background saves coalesce further edits while explicit navigation drains them.
Unknown outcomes retain request identity for reconciliation, and conflicts retain
local drafts. Ordinary native close has a scoped flush/acknowledgement gate.
Unsaved transient edits are not a durable process-crash journal.

The note editor caches canonical projections and size/reference summaries by
immutable ProseMirror node. Unchanged branches are reused rather than repeatedly
serializing the entire document while typing. Exact bounds and independent Rust
validation remain in place. Reference labels use inert node views and resolve only
when IDs or their metadata revision change. Editor teardown releases history/cache.
Hidden hosts stop observation/timers and release list metadata and saved bodies;
unresolved drafts remain available for close/retry handling.

The integration baseline profile schema was **18**; the integrated tree is at **20**
(19 adds Work environments, 20 the bounded checkpoint replay window). Frontend-only
development databases that used migration 14 for resources are incompatible; do not
silently accept them.

## Performance and native boundaries

Browser and panel startup graphs exclude heavy Work, XYFlow, LayerChart and Tiptap
code. Lazy graph budgets are enforced by `frame/bundle-budgets.json`; exact build
measurements appear in `frame/dist/bootstrap-report.json`. Complete static graph
sizes may include already-loaded shared/browser chunks, so they are not incremental
activation download sizes.

The first production artifact composition adds shared runtime/IPC code: measured
Browse/panel static JS is 293,953/136,712 bytes. Reviewed limits are 295,000/138,000;
affected existing lazy graphs allow the same shared increase. Work home/detail and
chart activation have explicit budgets. The gate also rejects `domain/work` from
startup. Charts retain LayerChart's standard SVG bar component. CSS blur is allowed
where useful and performance-appropriate; unused library styling does not require
a custom chart implementation. Execution approval and semantic artifact editing
bring the Work detail static graph to 121,330 JS / 14,479 CSS bytes; reviewed
limits are 123,000 / 16,000. These features remain outside Browse startup.

Native chrome/page separation, synchronous tab-presentation sentinels, fixed-raster
favicons, scoped event transport and production CSP remain in place. Appearance,
keyboard access, reduced motion and visual tokens use the existing interface system
in [design/system.md](design/system.md). Native material is not simulated with CSS
blur. Other agents' native runners have not been used for this qualification.

## Verification status and limits

At this checkpoint:

- `pnpm -C frame check`: passed, including 162 unit tests.
- `pnpm -C frame test:component`: passed, 37 WebKit tests.
- `pnpm -C frame build` and `cargo xtask check-frame-styles`: passed.
- Focused native library suites: app 326, core 354, desktop 104 with 2 ignored,
  store 268 with 1 ignored; 1,052 passing tests in total.
- The isolated macOS `Zephium Resources QA.app` was rebuilt with the empty Work
  host. Its `resource-ui-qa` feature is constrained to an isolated debug bundle.
- Native interactive appearance/focus/occlusion, Windows behavior and process-level
  memory/GPU qualification are not established by these tests.

The most recent `cargo xtask ci` run still stopped at the unchanged runtime
acquisition source-inventory gate:

```text
extension runtime acquisition boundary failed:
desktop/src/foreground_rendering_probe.rs external module resolves outside the
scanned Rust source inventory: desktop/foreground_probe_admission.rs
```

The gate and referenced runtime sources were unchanged by this frontend work.
No exemption was added. Focused tests do not make the full repository gate green.

Existing architectural debt remains visible: preference write readback still
bridges the absence of a committed preference projection; sidebar/tool coordination
and string UI commands remain transitional; the unified typed Browse surface/layout
migration is not complete. `knip-migration.ts` and `stylelint-migration.js` retain
explicit legacy exceptions. None of these is silently marked finished here.

Integration verification so far: frame check (168 tests), desktop library (106
passed, two environment skips), eight Work WebKit tests, chart rendering, build and
emitted-style checks pass. Native live qualification is still outstanding.
Public-discovery approval, transient activity ownership, browser promotion/takeover,
durable draft checkpoints, artifact editing and account workflows remain unfinished;
this checkpoint does not claim the complete Work product or end-to-end success.
