<script lang="ts">
  import { untrack, onMount } from "svelte";
  import { WorkEnvironmentContext, type WorkEnvironmentSession } from "$domain/work-environment";
  import { commandId, workSession, type WorkSession } from "$domain/work";
  import { resourceSession, type ResourceSession } from "$domain/resources";
  import type { TabView } from "$shared/ipc/bindings";
  import { preferences } from "$domain/preferences";
  import { loadNotes, loadNoteEditorHost } from "$features/notes";
  import { loadTasks } from "$features/tasks";
  import { IS_MAC } from "$shared/platform";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import LazyView from "$shared/ui/LazyView";
  import {
    Archive01Icon,
    ArrowLeft02Icon,
    FolderAddIcon,
    NoteAddIcon,
    Search01Icon,
    Settings02Icon,
    Tick02Icon,
  } from "../lib/icons";
  import { publicResearchQueryValid } from "../lib/public-research";
  import WorkChrome from "./chrome/WorkChrome.svelte";
  import TasksCapsule from "./chrome/TasksCapsule.svelte";
  import Composer from "./composer/Composer.svelte";
  import WorkTabPicker from "./WorkTabPicker.svelte";
  import Lift from "./Lift.svelte";
  import Inspector from "./Inspector.svelte";
  import { defaultSize } from "../lib/canvas-model";
  import { environmentPlan } from "../lib/project-environment-plan";
  import { environmentItems, environmentView } from "../lib/project-environment";
  import { environmentResults, type ResultReference } from "../lib/project-environment-results";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import type { CanvasView, CanvasItem } from "../lib/canvas-model";
  import type { WorkEnvironmentPanel } from "../lib/work-environment";
  import * as m from "$shared/i18n/messages";
  let {
    session,
    tabs,
    spaceName,
    profileLabel,
    aiEnabled = true,
    onreturn,
    onopen,
    onnewtab,
    onsettings,
    onopencitation,
  }: {
    session: WorkEnvironmentSession;
    tabs: readonly TabView[];
    spaceName: string;
    profileLabel: string;
    aiEnabled?: boolean;
    onreturn: () => void;
    onopen: (id: string) => void;
    onnewtab: () => void;
    onsettings: () => void;
    onopencitation?: (url: string) => void;
  } = $props();
  const context = untrack(() => new WorkEnvironmentContext(session.profile));
  onMount(() => {
    void context.start();
    return () => context.dispose();
  });
  onMount(() => {
    session.tabsIntroduced = true;
  });
  const notesHost = $derived(`environment:${session.space}`);
  let notes = $state.raw<ResourceSession | null>(
    untrack(() => resourceSession(session.profile, "note", notesHost)),
  );
  let taskList = $state.raw<ResourceSession | null>(
    untrack(() => resourceSession(session.profile, "task", `environment:${session.space}`)),
  );
  onMount(() => {
    const current = taskList;
    void current?.start();
    return () => current?.stopObserving();
  });
  let objectiveSession = $state.raw<WorkSession | null>(null);
  let inspected = $state<string | null>(null);
  let lifted = $state.raw<{ id: string; origin: DOMRect | null } | null>(null);
  let canvasRef = $state<{
    screenRect: (id: string) => DOMRect | null;
    selectionBounds: () => {
      x: number;
      y: number;
      width: number;
      height: number;
      ids: string[];
    } | null;
  }>();
  let selectionCount = $state(0);
  let cardHost = $state<HTMLElement>();
  let cardBounds = $state.raw<DOMRect | null>(null);
  $effect(() => {
    const element = cardHost;
    if (!element) return;
    const measure = () => (cardBounds = element.getBoundingClientRect());
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  });
  function openLift(id: string) {
    if (!items.some((item) => item.id === id)) return;
    chrome?.close();
    inspected = null;
    lifted = { id, origin: canvasRef?.screenRect(id) ?? null };
  }
  function liftSize(item: CanvasItem | undefined) {
    if (!item) return { width: 720, height: 520 };
    switch (item.type) {
      case "note":
        return { width: 760, height: 620 };
      case "tab":
        return { width: 520, height: 260 };
      case "objective":
        return { width: 640, height: 560 };
      case "responsibility":
        return { width: 480, height: 320 };
      default: {
        const base = defaultSize(item);
        return { width: Math.max(720, base.width + 200), height: Math.max(520, base.height + 160) };
      }
    }
  }
  let objectiveOpen = $state(false);
  let objectivePanel = $state<HTMLElement | undefined>();
  $effect(() => {
    if (objectiveOpen && objectivePanel) {
      inspected = null;
      objectivePanel.focus();
    }
  });
  let notesOpen = $state(false);
  let tasksOpen = $state(false);
  let title = $state("");
  let workQuery = $state("");
  let areaTitle = $state("");
  let objectivePending = $state(false);
  let composerFailure = $state<"limit" | "changed" | null>(null);
  let composerElement = $state<HTMLElement>();
  let composerHeight = $state(0);
  let chrome = $state<WorkChrome>();
  $effect(() => {
    const element = composerElement;
    if (!element) {
      composerHeight = 0;
      return;
    }
    const observer = new ResizeObserver(() => {
      composerHeight = element.getBoundingClientRect().height;
    });
    observer.observe(element);
    return () => observer.disconnect();
  });
  let archived = $state(false);
  const snapshot = $derived(session.snapshot);
  const baseItems = $derived(
    snapshot ? environmentItems(snapshot, tabs, context.notes, context.objectives) : [],
  );
  let expanded = $state<string | null>(null);
  let planGeometry = $state.raw<CanvasView>({
    positions: {},
    sizes: {},
    viewport: { x: 0, y: 0, zoom: 1 },
  });
  let inspectionExecution = $state<string | null>(null);
  let inspectCurrentPlan = $state(false);
  const scene = $derived(
    snapshot
      ? environmentPlan(snapshot, baseItems, context.objectives, context.plans, expanded)
      : { items: [], links: [], targets: new Map(), positions: {} },
  );
  const results = $derived(
    snapshot
      ? environmentResults(snapshot, scene.items, objectiveSession?.projection ?? null)
      : { items: scene.items, references: new Map<string, ResultReference>(), remaining: null },
  );
  const items = $derived(results.items);
  const liftedItem = $derived(items.find((item) => item.id === lifted?.id));
  const liftedElement = $derived(snapshot?.elements.find((element) => element.id === lifted?.id));
  const authoritative = $derived(new Set(snapshot?.elements.map((element) => element.id) ?? []));
  let resultSelection = $state<ResultReference | null>(null);
  let resultSource = $state<EvidenceReference | null>(null);
  const loadResult = () => import("./WorkResultInspector.svelte");
  async function inspectResult(id: string, source: EvidenceReference | null = null) {
    const reference = results.references.get(id);
    if (!reference) return;
    const current = workSession(session.profile);
    if (!current) return;
    objectiveSession = current;
    await current.start();
    if (await current.open(reference.objective)) {
      inspected = null;
      objectiveOpen = false;
      notesOpen = false;
      resultSelection = reference;
      resultSource = source;
    }
  }
  const savedResultPositions = $derived(
    Object.fromEntries(
      snapshot?.elements.flatMap((element) => {
        if (element.reference.kind !== "artifact") return [];
        const position =
          planGeometry.positions[
            `result:${element.reference.execution}:${element.reference.artifact}`
          ];
        return position ? [[element.id, position]] : [];
      }) ?? [],
    ),
  );
  const savedResultSizes = $derived(
    Object.fromEntries(
      snapshot?.elements.flatMap((element) => {
        if (element.reference.kind !== "artifact") return [];
        const size =
          planGeometry.sizes?.[
            `result:${element.reference.execution}:${element.reference.artifact}`
          ];
        return size ? [[element.id, size]] : [];
      }) ?? [],
    ),
  );
  const canvasView = $derived(
    snapshot
      ? {
          ...environmentView(snapshot),
          positions: {
            ...scene.positions,
            ...planGeometry.positions,
            ...savedResultPositions,
            ...environmentView(snapshot).positions,
          },
          sizes: { ...planGeometry.sizes, ...savedResultSizes, ...environmentView(snapshot).sizes },
        }
      : undefined,
  );
  const remoteView = $derived(
    session.remoteView
      ? {
          sequence: session.remoteView.sequence,
          view: environmentView({ ...snapshot!, view: session.remoteView.view }),
        }
      : undefined,
  );
  async function saveResult(id: string) {
    const reference = results.references.get(id);
    const position = planGeometry.positions[id];
    const size = planGeometry.sizes?.[id];
    if (!reference || !(await session.flushView())) return;
    if (!(await session.edit({ kind: "add", reference, area: null }))) return;
    const current = session.snapshot;
    const element = current?.elements.find(
      (element) =>
        element.reference.kind === "artifact" &&
        element.reference.objective === reference.objective &&
        element.reference.execution === reference.execution &&
        element.reference.artifact === reference.artifact,
    );
    if (current && element && position)
      session.checkpoint({
        ...current.view,
        placements: [
          ...current.view.placements.filter((place) => place.element !== element.id),
          {
            element: element.id,
            x: Math.round(position.x),
            y: Math.round(position.y),
            width: size?.width ?? 480,
            height: size?.height ?? 360,
          },
        ],
      });
  }
  async function inspectCanvas(id: string) {
    if (results.references.has(id)) {
      await inspectResult(id);
      return;
    }
    resultSelection = null;
    const target = scene.targets.get(id);
    if (!target) {
      inspected = id;
      return;
    }
    const current = workSession(session.profile);
    if (!current) return;
    objectiveSession = current;
    await current.start();
    if (await current.open(target.objective)) {
      inspectionExecution = target.execution;
      inspectCurrentPlan = target.execution === null;
      objectiveOpen = true;
    }
  }
  const element = $derived(snapshot?.elements.find((element) => element.id === inspected));
  const item = $derived(items.find((item) => item.id === inspected));
  const busy = $derived(!!session.pending || session.loading || objectivePending);
  const attachedTabs = $derived(
    snapshot?.elements.flatMap((element) =>
      element.reference.kind === "browser" ? [element.reference.tab] : [],
    ) ?? [],
  );
  const loadCanvas = () => import("./WorkCanvas.svelte");
  const loadInteraction = () => import("./WorkInteraction.svelte");
  const loadDetail = () => import("./WorkObjectiveInspector.svelte");
  $effect(() => {
    const current = snapshot;
    const focused = inspected;
    if (current) untrack(() => context.update(current, focused ?? undefined));
  });
  $effect(() => {
    const current = session;
    untrack(() => current.snapshot?.id);
    return () => objectiveSession?.stopObserving();
  });
  async function attach(ids: string[]) {
    for (const tab of ids) {
      if (!tabs.some((candidate) => candidate.id === tab)) continue;
      if (!(await session.edit({ kind: "add", reference: { kind: "browser", tab }, area: null })))
        break;
    }
  }
  async function attachNote() {
    const environmentId = session.snapshot?.id;
    const current = notes;
    if (
      !current ||
      !(await current.flush()) ||
      !current.record ||
      environmentId !== session.snapshot?.id
    )
      return;
    await session.edit({
      kind: "add",
      reference: { kind: "resource", resource: current.record.id },
      area: null,
    });
  }
  async function createNote() {
    const current = notes;
    if (!current) return;
    await current.start();
    await current.create(m.note_untitled());
    if (current !== notes || !current.record) return;
    await attachNote();
    chrome?.close();
    notesOpen = true;
  }
  async function inspectReference() {
    inspectionExecution = null;
    inspectCurrentPlan = false;
    const reference = element?.reference;
    if (!reference) return;
    if (reference.kind === "browser") {
      if (tabs.some((tab) => tab.id === reference.tab)) onopen(reference.tab);
    } else if (reference.kind === "resource") {
      const current = notes;
      if (!current) return;
      await current.start();
      await current.open(reference.resource);
      notesOpen = true;
    } else {
      const current = workSession(session.profile);
      if (!current) return;
      objectiveSession = current;
      await current.start();
      if (await current.open(reference.objective)) objectiveOpen = true;
    }
  }
  async function continueObjective(objective: string) {
    const current = workSession(session.profile);
    if (!current) return;
    objectiveSession = current;
    await current.start();
    if (await current.open(objective)) {
      objectiveOpen = false;
      inspected = null;
    }
  }
  async function createObjective() {
    if (!session.composer.trim() || objectivePending || busy) return;
    const current = workSession(session.profile);
    if (!current) return;
    const submission = session.objectiveSubmission ?? {
      objective: session.composer.trim(),
      command: commandId(),
      research: session.publicResearch,
      attached: false,
    };
    composerFailure =
      submission.research && !publicResearchQueryValid(submission.objective) ? "limit" : null;
    if (composerFailure) return;
    session.objectiveSubmission = submission;
    inspectionExecution = null;
    inspectCurrentPlan = false;
    objectivePending = true;
    objectiveSession = current;
    try {
      await current.start();
      if (!session.objectiveToAttach) {
        const pending = current.pending;
        const created = pending
          ? pending.kind === "author" &&
            pending.command.intent.kind === "create" &&
            pending.command.command === submission.command &&
            pending.command.intent.objective === submission.objective &&
            (await current.retry())
          : await current.create(submission.objective, submission.command);
        if (!created) {
          if (!current.pending) session.objectiveSubmission = null;
          return;
        }
        session.objectiveToAttach = current.selected;
      }
      const objectiveId = session.objectiveToAttach;
      if (!objectiveId) return;
      if (!submission.attached) {
        const attached = session.snapshot?.elements.some(
          (element) =>
            element.reference.kind === "objective" && element.reference.objective === objectiveId,
        );
        if (
          !attached &&
          !(await session.edit({
            kind: "add",
            reference: { kind: "objective", objective: objectiveId },
            area: null,
          }))
        )
          return;
        submission.attached = true;
      }
      if (!(await current.open(objectiveId))) return;
      const basis = current.projection?.work;
      if (current.selected !== objectiveId || basis?.id !== objectiveId) return;
      if (submission.research && basis.objective !== submission.objective) {
        composerFailure = "changed";
        return;
      }
      objectiveOpen = false;
      session.composer = "";
      session.objectiveToAttach = null;
      session.objectiveSubmission = null;
      if (submission.research) await current.readPublic();
      else
        await current.operations.begin({
          kind: "plan",
          request: { version: 1, work: objectiveId, expected_revision: basis.revision },
        });
    } finally {
      objectivePending = false;
    }
  }
  function checkpoint(view: CanvasView) {
    if (!snapshot || session.loading) return;
    const ids = Object.keys(view.positions).filter(
      (id) =>
        scene.targets.has(id) ||
        (results.references.has(id) && !snapshot.elements.some((element) => element.id === id)),
    );
    const positions = Object.fromEntries(
      [
        ...Object.entries(planGeometry.positions),
        ...ids.map((id) => [id, view.positions[id]!] as const),
      ].slice(-128),
    );
    const sizes = Object.fromEntries(
      [
        ...Object.entries(planGeometry.sizes ?? {}),
        ...ids.flatMap((id) => (view.sizes?.[id] ? [[id, view.sizes[id]] as const] : [])),
      ].slice(-128),
    );
    if (
      JSON.stringify(positions) !== JSON.stringify(planGeometry.positions) ||
      JSON.stringify(sizes) !== JSON.stringify(planGeometry.sizes)
    )
      planGeometry = { ...view, positions, sizes };
    const next = {
      revision: snapshot.view.revision,
      x: Math.round(view.viewport.x),
      y: Math.round(view.viewport.y),
      zoom_milli: Math.round(view.viewport.zoom * 1000),
      areas: snapshot.areas.flatMap((area) => {
        const place = view.areas?.[area.id];
        const previous = snapshot.view.areas?.find((entry) => entry.area === area.id);
        const source = place ?? previous;
        return source
          ? [
              {
                area: area.id,
                x: Math.round(source.x),
                y: Math.round(source.y),
                width: Math.round(source.width),
                height: Math.round(source.height),
              },
            ]
          : [];
      }),
      placements: snapshot.elements.map((element) => {
        const point = view.positions[element.id] ?? { x: 0, y: 0 };
        const previous = snapshot.view.placements.find((place) => place.element === element.id);
        return {
          element: element.id,
          x: Math.round(point.x),
          y: Math.round(point.y),
          width: view.sizes?.[element.id]?.width ?? previous?.width ?? 280,
          height: view.sizes?.[element.id]?.height ?? previous?.height ?? 160,
        };
      }),
    };
    if (
      JSON.stringify({ ...next, revision: "" }) !==
      JSON.stringify({ ...(session.viewDraft?.view ?? snapshot.view), revision: "" })
    )
      session.checkpoint(next);
  }
  async function closeNotes() {
    if (!notes || (await notes.flush())) {
      notesOpen = false;
      notes?.stopObserving();
    }
  }
  const taskItems = $derived(taskList?.items ?? []);
  const tasksDone = $derived(taskItems.filter((task) => task.completed).length);
  const activeExecution = $derived(
    !!objectiveSession?.projection?.executions.some(
      (execution) =>
        ["running", "cancel_requested", "approved"].includes(execution.status) &&
        !objectiveSession?.projection?.interrupted.includes(execution.id),
    ),
  );
  const needsDecision = $derived(
    !!objectiveSession?.projection?.work.questions.some((question) => question.state === "active"),
  );
  const visibleWorks = $derived(
    session.works.filter(
      (work) =>
        (work.lifecycle === "archived") === archived &&
        work.title.toLocaleLowerCase().includes(workQuery.trim().toLocaleLowerCase()),
    ),
  );
  async function groupSelection() {
    const bounds = canvasRef?.selectionBounds();
    const current = snapshot;
    if (!bounds || !current || !areaTitle.trim()) return;
    if (!(await session.flushView())) return;
    if (!(await session.edit({ kind: "create_area", title: areaTitle.trim() }))) return;
    const created = session.snapshot?.areas.find(
      (area) => !current.areas.some((known) => known.id === area.id),
    );
    if (!created) return;
    for (const id of bounds.ids)
      if (!(await session.edit({ kind: "assign_area", element: id, area: created.id }))) return;
    const latest = session.snapshot;
    if (latest)
      session.checkpoint({
        ...latest.view,
        areas: [
          ...(latest.view.areas ?? []).filter((entry) => entry.area !== created.id),
          {
            area: created.id,
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
          },
        ],
      });
    areaTitle = "";
    chrome?.close();
  }
  function onPanelChange(panel: WorkEnvironmentPanel | null) {
    session.tabsIntroduced = true;
    if (panel === "notes") notesOpen = false;
  }
</script>

{#snippet tabPanel()}<WorkTabPicker
    {tabs}
    {spaceName}
    attachedTabIds={attachedTabs}
    pending={busy}
    openInBrowse
    onattach={(ids) => void attach(ids)}
    {onopen}
    {onnewtab}
  />{/snippet}
{#snippet switcher()}
  <div class="menu">
    <button type="button" class="menu-row" onclick={onreturn}>
      <span class="menu-icon"><Icon icon={ArrowLeft02Icon} /></span>{m.work_env_return()}
    </button>
    <div class="menu-separator"></div>
    <label class="menu-search">
      <Icon icon={Search01Icon} size={14} />
      <input
        type="search"
        placeholder={m.work_env_search_works()}
        bind:value={workQuery}
        aria-label={m.work_env_search_works()}
      />
    </label>
    <div class="menu-heading">{archived ? m.work_archived() : m.work_env_recent()}</div>
    <ul class="menu-list">
      {#each visibleWorks as work (work.id)}<li>
          <button
            type="button"
            class="menu-row"
            class:selected={snapshot?.id === work.id}
            disabled={busy}
            onclick={() => {
              inspected = null;
              void session.open(work.id);
              chrome?.close();
            }}
            ><span class="menu-check"
              >{#if snapshot?.id === work.id}<Icon icon={Tick02Icon} size={14} />{/if}</span
            >{work.title}</button
          >
        </li>{:else}<li class="menu-empty">{m.work_env_no_works()}</li>{/each}
      {#if session.next && session.works.length < 256}<li>
          <button type="button" class="menu-row quiet" onclick={() => void session.reload(true)}
            >{m.resource_more()}</button
          >
        </li>{/if}
    </ul>
    <div class="menu-separator"></div>
    <form
      class="menu-create"
      onsubmit={(event) => {
        event.preventDefault();
        if (title.trim())
          void session.create(title.trim()).then((okay) => {
            if (okay) {
              title = "";
              chrome?.close();
            }
          });
      }}
    >
      <input
        aria-label={m.work_env_work_title()}
        bind:value={title}
        maxlength="128"
        placeholder={m.work_env_new_work()}
        disabled={busy}
      /><Button type="submit" size="compact" disabled={busy || !title.trim()}
        >{m.work_env_create()}</Button
      >
    </form>
    <button type="button" class="menu-row quiet" onclick={() => (archived = !archived)}>
      <span class="menu-icon"><Icon icon={Archive01Icon} /></span>{archived
        ? m.work_env_recent()
        : m.work_archived()}
    </button>
    {#if snapshot}<button
        type="button"
        class="menu-row quiet"
        disabled={busy}
        onclick={() =>
          void session.edit({
            kind: "set_lifecycle",
            lifecycle: snapshot.lifecycle === "active" ? "archived" : "active",
          })}
        >{snapshot.lifecycle === "active" ? m.work_env_archive() : m.work_env_restore()}</button
      >{/if}
  </div>
{/snippet}
{#snippet createPanel()}
  <div class="menu">
    <button type="button" class="menu-row" disabled={busy} onclick={() => void createNote()}>
      <span class="menu-icon"><Icon icon={NoteAddIcon} /></span>{m.work_env_new_note()}
    </button>
    <div class="menu-separator"></div>
    <div class="menu-heading">{m.work_env_new_area()}</div>
    <form
      class="menu-create"
      onsubmit={(event) => {
        event.preventDefault();
        if (areaTitle.trim())
          void session.edit({ kind: "create_area", title: areaTitle.trim() }).then((okay) => {
            if (okay) {
              areaTitle = "";
              chrome?.close();
            }
          });
      }}
    >
      <input
        aria-label={m.work_env_new_area()}
        bind:value={areaTitle}
        maxlength="128"
        placeholder={m.work_env_area()}
        disabled={busy}
      /><Button type="submit" size="compact" disabled={busy || !areaTitle.trim()}
        >{m.work_env_create()}</Button
      >
    </form>
  </div>
{/snippet}
{#snippet areaPanel()}
  <div class="menu">
    <div class="menu-heading">{m.work_env_new_area_hint()}</div>
    <form
      class="menu-create"
      onsubmit={(event) => {
        event.preventDefault();
        if (!areaTitle.trim()) return;
        if (selectionCount > 0) {
          void groupSelection();
          return;
        }
        void session.edit({ kind: "create_area", title: areaTitle.trim() }).then((okay) => {
          if (okay) {
            areaTitle = "";
            chrome?.close();
          }
        });
      }}
    >
      <span class="menu-icon"><Icon icon={FolderAddIcon} /></span>
      <input
        aria-label={m.work_env_new_area()}
        bind:value={areaTitle}
        maxlength="128"
        placeholder={m.work_env_area()}
        disabled={busy}
      /><Button type="submit" size="compact" disabled={busy || !areaTitle.trim()}
        >{selectionCount > 0
          ? m.work_env_group_selection({ count: selectionCount })
          : m.work_env_create()}</Button
      >
    </form>
    {#if snapshot?.areas.length}<div class="menu-heading">{m.work_env_area()}</div>
      <ul class="menu-list">
        {#each snapshot.areas as area (area.id)}<li class="menu-row static">{area.title}</li>{/each}
      </ul>{/if}
  </div>
{/snippet}
{#snippet notesPanel()}
  <div class="notes-panel">
    <LazyView
      loader={loadNotes}
      loadingLabel={m.surface_loading()}
      failureLabel={m.surface_render_failed()}
      retryLabel={m.surface_retry()}
      >{#snippet children(Notes)}<Notes
          profile={session.profile}
          host={notesHost}
        />{/snippet}</LazyView
    >
  </div>
  <div class="menu-footer">
    <Button
      size="compact"
      disabled={busy ||
        !notes?.record ||
        snapshot?.elements.some(
          (element) =>
            element.reference.kind === "resource" &&
            element.reference.resource === notes?.record?.id,
        )}
      onclick={() => void attachNote()}>{m.work_env_attach_note()}</Button
    >
  </div>
{/snippet}
{#snippet profilePanel()}<div class="menu">
    <div class="menu-identity">
      <span class="menu-avatar">{profileLabel.slice(0, 1).toLocaleUpperCase()}</span>
      <span class="menu-identity-text"
        ><strong>{profileLabel}</strong><span>{m.work_env_local_profile()}</span></span
      >
    </div>
    <div class="menu-separator"></div>
    <div class="menu-heading">{m.work_env_provider()}</div>
    <div class="menu-row static">OpenAI · GPT-5.6 Luna</div>
    <div class="menu-heading">{m.work_env_usage()}</div>
    <div class="menu-row quiet static">{m.work_env_usage_none()}</div>
    <div class="menu-separator"></div>
    <label class="menu-row toggle"
      ><span>{m.work_env_ai_enabled()}</span><input
        type="checkbox"
        role="switch"
        checked={preferences.value("ai.enabled") !== "false"}
        disabled={preferences.saving()}
        onchange={(event) =>
          void preferences.set("ai.enabled", String(event.currentTarget.checked))}
      /></label
    >
    <label class="menu-row toggle"
      ><span>{m.work_env_work_enabled()}</span><input
        type="checkbox"
        role="switch"
        checked={preferences.value("work.enabled") !== "false"}
        disabled={preferences.saving()}
        onchange={(event) =>
          void preferences.set("work.enabled", String(event.currentTarget.checked))}
      /></label
    >
    {#if preferences.saveFailed()}<p class="menu-status" role="status">
        {m.work_env_setting_failed()}
      </p>{/if}
    <div class="menu-separator"></div>
    <button type="button" class="menu-row" onclick={onsettings}>
      <span class="menu-icon"><Icon icon={Settings02Icon} /></span>{m.work_env_settings()}
    </button>
  </div>{/snippet}
{#snippet tasksPanel()}
  <div class="tasks-panel">
    <LazyView
      loader={loadTasks}
      loadingLabel={m.surface_loading()}
      failureLabel={m.surface_render_failed()}
      retryLabel={m.surface_retry()}
      >{#snippet children(Tasks)}<Tasks
          profile={session.profile}
          host={`environment:${session.space}`}
          onclose={() => (tasksOpen = false)}
        />{/snippet}</LazyView
    >
  </div>
{/snippet}
{#snippet composerAbove()}
  {#if !objectiveOpen && objectiveSession?.projection && snapshot?.elements.some((element) => element.reference.kind === "objective" && element.reference.objective === objectiveSession?.selected)}
    <LazyView
      loader={loadInteraction}
      loadingLabel={m.surface_loading()}
      failureLabel={m.surface_render_failed()}
      retryLabel={m.surface_retry()}
      >{#snippet children(Interaction)}
        <Interaction
          session={objectiveSession!}
          ondetails={(execution) => {
            inspectionExecution = execution ?? null;
            inspectCurrentPlan = !execution;
            objectiveOpen = true;
          }}
        />{/snippet}</LazyView
    >
  {/if}
  {#if composerFailure}<p class="composer-alert" role="alert">
      {composerFailure === "limit"
        ? m.work_env_public_query_limit()
        : m.work_env_public_query_changed()}
    </p>{/if}
  {#if objectiveSession?.failure}<p class="composer-alert" role="status">
      {m.work_request_failed({ reason: objectiveSession.failure })}
    </p>{/if}
{/snippet}
{#snippet composerFooter()}
  <button
    type="button"
    class="mode"
    class:on={session.publicResearch}
    aria-pressed={session.publicResearch}
    disabled={objectivePending || !!session.objectiveSubmission}
    onclick={() => {
      session.publicResearch = !session.publicResearch;
      composerFailure = null;
    }}>{m.work_env_public_research()}</button
  >
  <span class="disclosure"
    >{session.publicResearch ? m.work_env_public_disclosure() : m.work_planning_disclosure()}</span
  >
{/snippet}

<div
  class="environment"
  style:--work-header-height="52px"
  style:--work-header-inset-start={IS_MAC ? "84px" : "12px"}
>
  <div class="canvas-card" bind:this={cardHost}>
    {#if snapshot}{#key snapshot.id}
        <LazyView
          loader={loadCanvas}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(Canvas)}<Canvas
              {items}
              links={scene.links}
              areas={snapshot.areas}
              initialView={canvasView}
              {remoteView}
              {authoritative}
              expose={(api) => (canvasRef = api)}
              fitBottomInset={composerHeight}
              oninspect={(id: string) => {
                if (scene.targets.has(id) || results.references.has(id)) void inspectCanvas(id);
              }}
              onopen={openLift}
              onselectionchange={(ids: string[]) => {
                selectionCount = ids.filter((id) => authoritative.has(id)).length;
                if (!ids.length) inspected = null;
              }}
              onareachange={(id: string, area: string | null) =>
                void session.edit({ kind: "assign_area", element: id, area })}
              onevidence={(id: string, source: EvidenceReference) => void inspectResult(id, source)}
              onaction={(id: string, action?: string) => {
                if (action === "remove") {
                  const element = snapshot?.elements.find((element) => element.id === id);
                  if (element)
                    void session.edit({ kind: "remove", element: element.id }).then((okay) => {
                      if (okay && inspected === id) inspected = null;
                    });
                  return;
                }
                if (action === "inspect") {
                  void inspectCanvas(id);
                  return;
                }
                const reference = results.references.get(id);
                if (reference) {
                  void saveResult(id);
                  return;
                }
                expanded =
                  items.find((item) => item.id === id)?.actionLabel === m.work_env_collapse_plan()
                    ? null
                    : id;
              }}
              onviewchange={checkpoint}
            />{/snippet}</LazyView
        >
        {#if snapshot.elements.length === 0}<div class="welcome">
            <h1>{snapshot.title}</h1>
            <p>{aiEnabled ? m.work_env_manual_hint() : m.work_env_ai_off_hint()}</p>
          </div>{/if}
      {/key}{:else}<div class="welcome">
        <p>{session.loading ? m.surface_loading() : m.work_env_preparing()}</p>
      </div>{/if}
    {#if session.failure || session.pending || session.viewDraft}<div class="status" role="status">
        <span
          >{session.failure
            ? m.work_request_failed({ reason: session.failure })
            : session.pending
              ? m.work_env_pending()
              : m.work_env_unsaved_view()}</span
        >{#if session.delivery === "unknown"}<Button
            size="compact"
            onclick={() => void session.retry()}>{m.work_reconcile()}</Button
          >{:else if session.delivery === "conflict" && session.viewDraft}<Button
            size="compact"
            onclick={() => void session.discardView()}>{m.work_env_discard_view()}</Button
          >{:else if session.failure}<Button size="compact" onclick={() => void session.refresh()}
            >{m.work_env_reload()}</Button
          >{/if}
      </div>{/if}
    {#if results.remaining}<div class="results-disclosure">
        <Button
          size="compact"
          onclick={() => {
            if (!results.remaining) return;
            inspectionExecution = results.remaining.execution;
            inspectCurrentPlan = false;
            objectiveOpen = true;
          }}>{m.work_env_other_results({ count: results.remaining.count })}</Button
        >
      </div>{/if}
    {#if element && item && !objectiveOpen && !notesOpen && !resultSelection && !lifted}<div
        class="inspector"
      >
        <Inspector
          {element}
          {item}
          areas={snapshot?.areas ?? []}
          {busy}
          openLabel={element.reference.kind === "browser"
            ? m.work_env_open_browse()
            : m.work_env_open()}
          openDisabled={element.reference.kind === "browser" &&
            !tabs.some(
              (tab) => element.reference.kind === "browser" && tab.id === element.reference.tab,
            )}
          onopen={() => {
            if (element.reference.kind === "browser") void inspectReference();
            else openLift(element.id);
          }}
          onarea={(area) => void session.edit({ kind: "assign_area", element: element.id, area })}
          oncontinue={element.reference.kind === "objective"
            ? () => {
                if (element.reference.kind === "objective")
                  void continueObjective(element.reference.objective);
              }
            : undefined}
          onremove={() =>
            void session.edit({ kind: "remove", element: element.id }).then((okay) => {
              if (okay) inspected = null;
            })}
          onclose={() => (inspected = null)}
        />
      </div>{/if}
    {#if resultSelection && objectiveSession && !objectiveOpen && !notesOpen}<section
        class="inspector"
        aria-label={m.work_env_results()}
      >
        <Button size="compact" onclick={() => (resultSelection = null)}>{m.work_env_close()}</Button
        >
        <LazyView
          loader={loadResult}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(
            Result,
          )}{#key `${resultSelection!.objective}:${resultSelection!.execution}:${resultSelection!.artifact}`}<Result
                session={objectiveSession!}
                reference={resultSelection!}
                source={resultSource}
                onopen={onopencitation}
              />{/key}{/snippet}</LazyView
        >
      </section>{/if}
    {#if notesOpen}<section class="detail" aria-label={m.work_env_notes()}>
        <Button onclick={() => void closeNotes()}>{m.work_env_close()}</Button
        >{@render notesPanel()}
      </section>{/if}
    {#if objectiveOpen && objectiveSession}<section
        bind:this={objectivePanel}
        tabindex="-1"
        class="detail"
        aria-label={m.work_env_objective()}
      >
        <Button onclick={() => (objectiveOpen = false)}>{m.work_env_back_canvas()}</Button><LazyView
          loader={loadDetail}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(
            Detail,
          )}{#key `${objectiveSession?.selected}:${inspectionExecution}:${inspectCurrentPlan}`}<Detail
                initialExecution={inspectionExecution}
                showCurrentPlan={inspectCurrentPlan}
                session={objectiveSession!}
                attached={snapshot?.elements.map((element) => element.reference) ?? []}
                {onopencitation}
                onattach={(reference) => void session.edit({ kind: "add", reference, area: null })}
              />{/key}{/snippet}</LazyView
        >
      </section>{/if}
  </div>
  {#if lifted && cardBounds}
    <Lift
      origin={lifted.origin}
      bounds={cardBounds}
      preferred={liftSize(liftedItem)}
      title={liftedItem?.title ?? ""}
      onclose={() => (lifted = null)}
    >
      {#if liftedElement?.reference.kind === "resource"}
        <LazyView
          loader={loadNoteEditorHost}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(Host)}<Host
              profile={session.profile}
              host={notesHost}
              id={liftedElement.reference.kind === "resource"
                ? liftedElement.reference.resource
                : ""}
            />{/snippet}</LazyView
        >
      {:else if liftedElement?.reference.kind === "objective" && objectiveSession}
        <LazyView
          loader={loadDetail}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(Detail)}<Detail
              initialExecution={null}
              showCurrentPlan
              session={objectiveSession!}
              attached={snapshot?.elements.map((element) => element.reference) ?? []}
              {onopencitation}
              onattach={(reference) => void session.edit({ kind: "add", reference, area: null })}
            />{/snippet}</LazyView
        >
      {:else if liftedItem?.artifact}
        <div class="lift-result">
          <h2>{liftedItem.title}</h2>
          {#if results.references.get(liftedItem.id) && objectiveSession}
            <LazyView
              loader={loadResult}
              loadingLabel={m.surface_loading()}
              failureLabel={m.surface_render_failed()}
              retryLabel={m.surface_retry()}
              >{#snippet children(Result)}<Result
                  session={objectiveSession!}
                  reference={results.references.get(liftedItem!.id)!}
                  source={null}
                  onopen={onopencitation}
                />{/snippet}</LazyView
            >
          {/if}
        </div>
      {:else if liftedItem}
        <div class="lift-plain">
          <span class="kind">{liftedItem.kind}</span>
          <h2>{liftedItem.title}</h2>
          <p>{liftedItem.detail}</p>
          <p>{liftedItem.status}</p>
          {#if liftedElement?.reference.kind === "browser"}<Button
              size="compact"
              onclick={() => {
                lifted = null;
                void inspectReference();
              }}>{m.work_env_open_browse()}</Button
            >{/if}
        </div>
      {/if}
    </Lift>
  {/if}
  <WorkChrome
    bind:this={chrome}
    {spaceName}
    workTitle={snapshot?.title ?? m.work_env_default_title()}
    {profileLabel}
    initialTabsOpen={!session.tabsIntroduced}
    panels={{
      tabs: tabPanel,
      switcher,
      create: createPanel,
      notes: notesPanel,
      area: areaPanel,
      profile: profilePanel,
    }}
    {onreturn}
    onpanelchange={onPanelChange}
  />
  <TasksCapsule
    done={tasksDone}
    total={taskItems.length}
    active={activeExecution}
    {needsDecision}
    open={tasksOpen}
    panel={tasksPanel}
    onopenchange={(open) => (tasksOpen = open)}
  />
  {#if aiEnabled}
    <div class="composer-dock">
      <Composer
        bind:ref={composerElement}
        bind:value={session.composer}
        placeholder={m.work_env_prompt()}
        disabled={objectivePending || !!session.objectiveSubmission}
        {busy}
        above={composerAbove}
        footer={composerFooter}
        onsubmit={() => void createObjective()}
      />
    </div>
  {/if}
</div>

<style>
  .environment {
    position: relative;
    inline-size: 100%;
    block-size: 100%;
    min-inline-size: 0;
    min-block-size: 0;
    overflow: hidden;
    color: var(--color-text);
    font-size: var(--text-body);
  }

  .canvas-card {
    position: absolute;
    inset: var(--work-header-height) 8px 8px;
    border-radius: var(--content-radius);
    background: var(--color-canvas);
    box-shadow: inset 0 0 0 1px var(--color-border);
    overflow: hidden;
  }

  .composer-dock {
    position: absolute;
    inset-block-end: 0;
    inset-inline-start: 50%;
    inline-size: min(680px, calc(100% - 200px));
    translate: -50% 0;
    z-index: 30;
  }

  .welcome {
    position: absolute;
    inset: 36% 24px auto;
    pointer-events: none;
    text-align: center;
  }

  .welcome h1 {
    margin-block: 0 8px;
    font-size: var(--text-title);
    font-weight: 500;
    letter-spacing: -0.01em;
  }

  .welcome p {
    margin: 0;
    color: var(--color-muted);
  }

  .status {
    position: absolute;
    inset-inline-end: 16px;
    inset-block-end: 16px;
    display: flex;
    align-items: center;
    gap: 12px;
    max-inline-size: calc(100% - 32px);
    padding: 8px 12px;
    border-radius: var(--radius-control);
    background: var(--color-menu);
    backdrop-filter: blur(10px);
    box-shadow: var(--shadow-popover);
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .results-disclosure {
    position: absolute;
    inset-block-start: 16px;
    inset-inline-end: 16px;
  }

  .lift-result h2,
  .lift-plain h2 {
    margin: 0 0 12px;
    font-size: var(--text-title);
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .lift-plain .kind {
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .lift-plain p {
    margin: 0 0 8px;
    color: var(--color-muted);
  }

  .inspector,
  .detail {
    position: absolute;
    inset-block: 16px;
    inset-inline-end: 16px;
    inline-size: min(360px, calc(100% - 32px));
    box-sizing: border-box;
    overflow: auto;
    padding: 16px;
    border-radius: var(--radius-menu);
    background: var(--color-menu);
    backdrop-filter: blur(14px) saturate(1.2);
    box-shadow: var(--shadow-popover);
    z-index: 20;
  }

  .detail {
    inline-size: min(520px, calc(100% - 32px));
  }

  .menu {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .menu-row {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    inline-size: 100%;
    min-block-size: 34px;
    padding: 6px 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-instant) ease;
  }

  .menu-row:disabled {
    opacity: 0.5;
  }

  .menu-row:not(.static):hover:not(:disabled),
  .menu-row.selected {
    background: var(--color-control-hover);
  }

  .menu-row.quiet {
    color: var(--color-muted);
  }

  .menu-row.static {
    cursor: default;
  }

  .menu-row.toggle {
    justify-content: space-between;
  }

  .menu-icon,
  .menu-check {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 16px;
    color: var(--color-muted);
  }

  .menu-separator {
    block-size: 1px;
    margin: 6px -2px;
    background: var(--color-border);
  }

  .menu-heading {
    padding: 8px 10px 4px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .menu-list {
    list-style: none;
    margin: 0;
    padding: 0;
    max-block-size: 280px;
    overflow: auto;
  }

  .menu-empty,
  .menu-status {
    padding: 8px 10px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .menu-search {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    block-size: 32px;
    border-radius: var(--radius-control-compact);
    background: var(--color-field);
    color: var(--color-muted);
  }

  .menu-search input,
  .menu-create input {
    flex: 1;
    min-inline-size: 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    outline: none;
  }

  .menu-create {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 4px 4px 10px;
    border-radius: var(--radius-control-compact);
    background: var(--color-field);
  }

  .menu-identity {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
  }

  .menu-avatar {
    display: grid;
    place-items: center;
    inline-size: 36px;
    block-size: 36px;
    border-radius: 50%;
    background: var(--color-control);
    box-shadow: var(--shadow-control);
    font-weight: 600;
  }

  .menu-identity-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .menu-identity-text span {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .menu-footer {
    display: flex;
    justify-content: flex-end;
    padding: 8px 4px 0;
  }

  .notes-panel {
    display: flex;
    block-size: 440px;
    min-inline-size: 0;
  }

  .tasks-panel {
    display: flex;
    block-size: min(60vh, 520px);
    min-inline-size: 0;
  }

  .composer-alert {
    margin: 0;
    padding: 10px 14px;
    border-radius: var(--radius-control);
    background: var(--color-menu);
    backdrop-filter: blur(10px);
    box-shadow: var(--shadow-popover);
    color: var(--color-warning);
    font-size: var(--text-label);
  }

  .mode {
    flex: none;
    padding: 2px 8px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    font-weight: 500;
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      color var(--motion-fast) var(--ease-smooth);
  }

  .mode:hover:not(:disabled) {
    background: var(--color-fill-hover);
  }

  .mode.on {
    background: var(--color-fill-active);
    color: var(--color-text);
  }

  .disclosure {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
