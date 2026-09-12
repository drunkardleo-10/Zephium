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

The normal IPC binding is `frame/src/shared/ipc/bindings.ts`, generated from Rust.
The copied `crates/zephium-ipc/bindings/work-v1.ts` has been removed. It was reference
material from the separate runtime machine, never a frontend build dependency.
There is no replacement Work wire schema in this frontend stream.

## Native browser and empty Work host

The sidebar Browse/Work switch requests `browser.work` or `browser.return` through
the existing command transport. Rust's `BrowserPage::Work` borrows the established
internal-surface layout. Native page WebViews are suppressed while the internal
surface is active. Returning to Browse uses the existing synchronous chrome
restoration and exact tab-identity checks. The mode control follows native
projection, not command admission alone.

`app/browser/WorkWorkspace.svelte` mounts only an empty lazy canvas with pan/zoom
controls. It has no Notes/Tasks composition, resource queries, authoring controls,
profile arrangement cache or execution state. Notes and Tasks remain independent
browser tools. The native shell unmounts the canvas when leaving Work.

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
unknown/resynchronizing states. These blocks are not mounted in the empty native
canvas. Action meaning, exact scope and request outcomes come from its host.
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

Resource persistence appends **profile migration 14** in this checkout. The
separate runtime stream may have its own later migrations; version/order agreement
is not established by this handoff. Shared merge areas include core/store ports,
Store actor and migrations, app API, IPC exports and desktop dispatch/close hooks.
The normal generated bindings reflect this checkout's Rust source, not a merged
runtime implementation.

## Performance and native boundaries

Browser and panel startup graphs exclude heavy Work, XYFlow, LayerChart and Tiptap
code. Lazy graph budgets are enforced by `frame/bundle-budgets.json`; exact build
measurements appear in `frame/dist/bootstrap-report.json`. Complete static graph
sizes may include already-loaded shared/browser chunks, so they are not incremental
activation download sizes.

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

Live Work projection admission, revision/request reconciliation, artifact effects,
evidence access, browser promotion and agent-resource authorization are absent from
this stream. The independent runtime owns those semantics. The delivered frontend
provides reusable UI capabilities and a native host, not the final Work product
composition. No commit, push or runtime-branch merge is implied by this document.
