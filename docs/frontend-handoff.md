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
| `features/notes` | Markdown notes: list, note view, lazy editor, panel body and `browser.notes` page |
| `features/tasks` | Inline capture with date/time reading, due-date sections, scheduling, multiple selection, keyboard operation and undo at panel, rail or page density; a full destination with list and board views, available through its lazy loaders |

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

## Notes

Notes are Markdown files the person owns, one `.md` file per note. The files are
the record; everything else describes them and can be rebuilt from them.

- **Storage.** `crates/zephium-notes` owns a profile's folder at
  `<app data>/notes/<profile>/Notes` and a SQLite index beside it
  (`notes/<profile>/index.sqlite`, never inside the folder, so moving or syncing
  the folder never carries a database). The index keeps identities (ULIDs),
  titles, previews, pins, `[[link]]` targets and an FTS5 index of titles and
  bodies. Losing it loses pins and identities, never a note.
- **Thread.** File work runs on its own `zephium-notes` thread, never the Store
  actor. Saves are atomic (temporary file, plain `fsync`, rename) and preserve a
  file's line endings, byte-order mark and permissions. Nothing follows a symbolic
  link or writes outside the folder. Files over 1 MiB or not UTF-8 are listed but
  read-only.
- **Outside edits.** A recursive watcher (FSEvents, ReadDirectoryChangesW,
  inotify) settles bursts for 250 ms. Events the index already explains, such as
  the browser's own saves, cost a `stat`, not a scan. Opening a profile reconciles
  changes made while the browser was closed; a moved file keeps its identity by
  inode or content.
- **Contract.** `note_call(expected_profile, NoteCall)` is routed by the shell
  after the same focused-profile check as `resource_call`; private profiles are
  refused. A write carries the content revision it edited (a hash of the bytes on
  disk) and returns `Conflict` with the file's current text rather than
  overwriting a change made elsewhere; replaying a write or a create (by
  `request_id`) never duplicates it. `NoteChanges` (`zephium:notes-changed`)
  names changed notes with their new revisions; `reset` is set only when titles,
  names or the folder changed, since those move where links lead. A session
  ignores events for its own saves. The main window always
  receives it; the launcher only while it is showing Notes, so a hidden
  launcher is never woken by a save. A surface that holds notes while hidden
  must list them again when shown rather than rely on events it missed.
- **Titles and names.** A note's title is its leading heading, else its file
  name. A file's name follows its title only while it still matches the title it
  was named after, so a name chosen in Finder or another editor is never changed.
  Trash is the folder's `.trash/`, emptied after 30 days. Launcher search covers
  note bodies.
- **Migration and deletion.** On first open, notes stored as ProseMirror JSON in
  the profile database become files under their old IDs, and the rows are
  retired. The resource store now refuses new note rows. Profile deletion
  releases the profile's notes, then erases `notes/<profile>` (renamed aside
  first) inside the journal-authorized purge.

The frontend reads and writes Markdown directly. `features/notes/lib/markdown`
parses with `marked` (GFM plus `[[wiki links]]` and footnotes) into the editor's
Tiptap schema and writes Markdown back:

- Blocks the editor has not changed are written back byte for byte.
- Changed blocks are rewritten with the delimiters they were read with, and each
  rewritten paragraph is re-parsed to prove it reads back as the same content.
- Anything the editor does not render (tables, HTML, footnotes, front matter,
  images) is kept verbatim as a raw node.
- Serialization is cached per block, so a keystroke rewrites one block.
- Code blocks labelled with a language are coloured by `lib/editor/highlight.ts`:
  a one-pass scanner (comments, strings, numbers, keywords) whose ranges go to
  the CSS Custom Highlight API, so no elements are added. It runs when a note
  opens and 300 ms after an edit, for changed blocks only; engines without the
  API show plain code.

`domain/notes` holds the session: autosave (600 ms idle, at least every 4 s while
typing, one background write at a time), idempotent retries, conflict choice, and
recreating a note whose file vanished while it was open. The panel (`NotesView`)
and the `browser.notes` page share the list, note view and lazy editor chunk.

For Work: a note is a Markdown string plus a `NoteSummary`. A canvas projection
should render with `MarkdownDocument` and `noteSchemaExtensions`, read-only, or
from `marked` tokens, and write through `note_call` with `base_revision`. It must
not keep its own copy of note content. Merge areas: `zephium-core` (`notes.rs`,
`ports/notes.rs`, two Store trait methods), shell commands `AttachNotes` and
`NoteCall`, `desktop/src/notes.rs`, profile deletion and the regenerated
bindings. No PROFILE migration was added.

## Tasks resources

Authoritative resource types live in `crates/zephium-core/src/resources.rs`.
Persistence lives in `crates/zephium-store/src/hub/resources.rs`, through the Store
actor. Native IPC supports bounded lists, reads and idempotent, revision-checked
mutations. Identity, revisions and saved state come from Rust.

Tasks contain title, description, due date, an optional due time, and pin state, plus a lifecycle
(`open`/`active`/`blocked`/`done`), who holds the next move and who created it
(`user`/`agent`), the page a task came from, and a manual sort key. `completed`
remains stored alongside `status` as the projection the listing column and query
filter are built from; validation keeps the two in step, and PROFILE migration 16
adopts both for tasks written before the lifecycle existed. Listing columns carry
everything a row draws, so a populated list costs one query rather than a fetch
per row. Search, pinning and soft trash/restore are implemented.

People and agents share one list: `assignee` says who is expected to act, not who
is permitted to, and an agent's own plan steps are execution state that never
enters it. The `work` field is reserved for the Work runtime track, which owns
Work identity and the rules binding a task to one; this tree assigns it no meaning
and enforces no reference. Notes are deliberate user knowledge resources, not
agent-system memory. Tasks are user resources, not Work plan nodes or execution
attempts. Agent-facing resource authorization is not implemented here.

Tasks no longer use the shared resource panel. `domain/resources/tasks.svelte.ts`
holds a task-shaped session that draws each row from intent and reconciles it
against native settlement, serialising writes per task and rebasing a field patch
onto whatever the record has become; an edit arriving from elsewhere is an update
to fold in, not a conflict to resolve. `features/tasks` owns the body — inline
capture, sectioning by due date, scheduling, keyboard operation and undo — at
`panel`, `rail` or `page` density, and a host supplies the session and the chrome.

A due time is `HH:MM` and only ever exists alongside a day; PROFILE migration 17
projects it. Manual position is a fixed-width decimal key written by dragging a
board card, with a resequence when a gap is spent. The board's drag is the shared
`pointer-drag` primitive, not an HTML5 drag and not a dependency; the sidebar's
tab gesture has not been migrated onto it and remains its own code.

Not implemented here: reminders (there is no notification plugin in the tree, so
a due time does not notify), recurrence, any grouping beyond scopes and search,
and a keyboard equivalent for board reordering — cards move by pointer only.

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

The 2026-09-25 extension store UI integration adds 147 bytes to shared lazy
graphs: NotesPage measures 290,005 versus 289,858 bytes and the notes domain
149,522 versus 149,375. Their JS caps were reviewed at 290,500 and 150,000;
browser startup remains at the 24-request cap without a new eager feature.

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
