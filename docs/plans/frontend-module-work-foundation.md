# Frontend modules and Work presentation handoff

Implementation checkpoint: 2026-09-12. This document replaces the earlier module
proposal. The enforced convention is `components/`, `lib/`, `tests/`; no `model/`,
`pages/` or `adapters/` directory template is required. The
[frontend contract](../frontend.md) and [progress record](frontend-architecture-progress.md)
distinguish this presentation foundation from remaining Browse/native/runtime work.

## Ownership and growth

A feature has a curated `index.ts`, rendered composition in `components/`, and
controllers, pure selectors and presentation types in `lib/`. Tests live in its
`tests/`. Header/footer/section names are components, not architectural layers.
Create a nested capability only when it has independently meaningful behavior.
Do not create empty directories, generic service bags or parallel entity stores.

Domain slices retain a few focused source files and a `tests/` directory. Session
modules own transient state shared by features with an explicit lifetime. Rust
owns execution, identities, persistence, permissions and legal transitions.

The new code follows the same rules:

| Module                          | Responsibility                                                                   | Public API                                              |
| ------------------------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------- |
| `features/work`                 | Canvas and Work presentation composition, local drafts, selection, view geometry | `loadWorkSurface`, `loadWorkCanvas`, presentation types |
| `shared/ui/data/Artifact`       | Inert semantic-content presentation, bounded inputs, source callbacks            | `Artifact`, `ArtifactView`                              |
| `shared/ui/data/Chart`          | Lazy SVG bar chart and original-decimal value table                              | `loadChart`, `ChartSeries`                              |
| `shared/ui/data/DocumentEditor` | Lazy, bounded paragraph/text Tiptap draft editor                                 | `loadDocumentEditor`                                    |
| `shared/ui/data/Evidence`       | Bounded historical excerpts, truncation and original byte count                  | `Evidence`, `EvidenceView`                              |
| `shared/ui/data/DataTable`      | Accessible paginated plain-prop table                                            | `DataTable`, table display types                        |
| `dev/`                          | Isolated authored trip, research and interruption examples                       | Separate preview entry only                             |

The Work composition consumes shared renderers. Shared controls never import Work.
The renderer contracts are **presentation values**, not copies of Work IPC schemas.
Chart owns its numerical presentation types. Artifact owns its evidence-button
references; the host resolves those references to runtime-owned citation identities.

## Stack and rendering boundaries

- Svelte 5, TypeScript, Vite, semantic tokens, existing Zephium controls and narrow
  Bits UI usage remain unchanged. Styling is Zephium-owned; no Shadcn theme is copied.
- Svelte XYFlow owns canvas interaction. Node IDs remain opaque. Dependency and
  reference links have distinct representations. Nodes cannot mint connections or
  delete semantic entities. Keyboard inspection and a complete list alternative
  remain available. Rendering is bounded to 500 items and 2,000 relationships.
- Node identity and user arrangement survive unrelated data updates. New cards avoid
  occupied default positions. Invalid saved coordinates/zoom are rejected. Settled
  positions (including keyboard movement) and viewport changes emit view callbacks;
  they do not update Work execution state.
- Offscreen culling is enabled from 100 items. In the measured WebKit 500-item
  restored-viewport case, rendering settled to 4 nodes. XYFlow still initially
  measures nodes; this is not evidence of reduced cold mount cost or native GPU use.
- LayerChart uses its SVG entry, loaded only when a chart is inspected. One bounded
  series is plotted at a time, with motion disabled. Original decimal strings remain
  in an accessible table. Nonfinite, out-of-range, underflowing or unsupported values
  and duplicate categories fall back to the data rather than a misleading plot.
- The existing lightweight table remains. TanStack Table is deferred until a real
  requirement justifies advanced table behavior.
- Tiptap loads only for editing. Its schema is document/paragraph/text, with bounded
  undo history and transaction limits. Generated document paragraphs are not an
  arbitrary rich-text schema. Images, links, HTML and styling are not editor payloads.
  Mount by document identity; incoming props do not overwrite a current draft.
  The host receives drafts and owns saved/conflict/unknown status. Destroying the
  component destroys the editor; removing editability does not fabricate an edit.

## Current surface behavior

`WorkSurfaceView` is a bounded, host-supplied display value. It contains an objective,
clarification controls, card/link presentations, artifacts, optional interruption
notice and explicitly described actions. It contains no worker pool, scheduler,
model router, native browser owner or synthetic runtime IDs.

Objective and clarification submission emit local UI intent callbacks. Actions
require a second explicit confirmation; replacing the view invalidates an open
confirmation. Scope and consequence come from the host. The host must map each
opaque action key to fixed runtime operands and the exact applicable revision.
Reusing a display key for different authority is not permitted by this UI contract.

The host supplies request state: ready, pending, rejected, conflict, unknown or
resynchronizing. Pending/conflict/unknown/resynchronizing block mutation controls.
Transport callback observation is bounded to 10 seconds and released on hide or
disposal. Rejection, timeout and lost observation require reconciliation; aborting
observation does not cancel an admitted native effect. Neither a
fulfilled callback nor an animation is a durable completion signal.

`active=false` unmounts canvas/inspector children while retaining local authoring
and arrangement state in the surface owner. There are no Work feature subscriptions,
pollers, workers or recurring timers. Imported JavaScript is still cached by the
WebView; unmounting is not unloading. A production host must retain drafts before
changing Work/profile identity or closing the view. Native autosave is not wired.

Artifact rendering covers document, table, comparison, chart, checklist, source
collection and browser-resource description. Unsupported content is explicit.
Checklists are read-only facts until a host supplies an exact artifact-edit workflow.
Browser cards have no decoded preview images, clickable model URLs or live WebViews.
The current copied contract provides a descriptive card, not a safe-frame capability.
Evidence displays escaped historical text, its truncation status and decimal byte
count. It does not navigate or grant access to a browser context.

## Development and verification

```sh
pnpm -C frame dev:work
# Open http://localhost:1421/work-preview.html
pnpm -C frame check
pnpm -C frame test:component
pnpm -C frame build
pnpm -C frame build:work
cargo xtask check-frame-styles
```

`dist-work-preview/` is a separate, ignored build artifact. It cannot import the
native transport. Production `dist/` rejects development modules and keeps Work,
XYFlow, LayerChart and Tiptap out of both native startup graphs. No native route,
CSP rule, Rust code or existing Browse surface is changed by this pass.

`work-bundle-budgets.json` bounds the complete static graph of each authored lazy
entry, including shared JavaScript and CSS. The entire preview distribution is
also capped at 1.1 MB of uncompressed JavaScript, including optional library chunks.
Budgets include modest explicit headroom over measured output. This is renderer
qualification; actual native Work-open latency, memory/GPU use and Windows behavior
remain integration measurements.

The deterministic scenarios exercise actual production components through props.
They have no network calls, native actions, execution engine or persistence. A
purchase approval and takeover are visibly unavailable without exact native scope.
They are presentation fixtures, not proof of contract compatibility or live workflows.

## Runtime integration remains a real engineering phase

The copied `crates/zephium-ipc/bindings/work-v1.ts` remains untracked and unmodified.
It was read as the semantic reference. The frontend build does not depend on that
local file, and the handoff must not commit or overwrite it.

At integration, the runtime agent must:

1. Regenerate/import the runtime-owned versioned definitions. The supplied V1 uses
   `work`, `command` and `expected_revision`, not the earlier conceptual envelope
   names. Authoring, planning, execution and evidence have distinct typed routes.
2. Implement projection admission, request correlation, stale/replay/unknown-result
   reconciliation and profile/Work/window scope. Bind the view callbacks to those
   routes without turning display state into authority.
3. Map executions against their approved plan revision, including historical plans,
   interrupted-owner facts, worker responsibility and conservative/unknown usage.
   Add exact plan amendment/review/edit workflows around these presentation blocks.
4. Supply real evidence reads, artifact review/edit receipts, native view-state and
   draft persistence. Confirm saved state through authoritative responses.
5. Supply bounded safe-frame references and actual browser promotion/release/takeover
   ownership before enabling those actions. Descriptive resource URLs are insufficient.
6. Validate runtime fixtures and exercise real vertical workflows, cancellation,
   uncertain effects, restart, native focus, CSP and macOS/Windows resource cleanup.

The older Browse preference/readback, unified surface/layout projection and global
style migration items remain separately tracked. This presentation handoff does not
silently mark those architectural migrations complete or remove their existing gates.
