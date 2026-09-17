<script lang="ts">
  import { untrack, onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { WorkEnvironmentContext, type WorkEnvironmentSession } from "$domain/work-environment";
  import { commandId, workSession, type WorkSession } from "$domain/work";
  import { resourceSession, type ResourceSession } from "$domain/resources";
  import type {
    TabView,
    WorkAccountEffectV1,
    WorkEnvironmentSnapshot,
    WorkExecutionFact,
    WorkRuntimeProjection,
  } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import { layout } from "$domain/layout";
  import { workPane, type WorkPaneRect, type WorkPaneTarget } from "$domain/work-pane";
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
  import WorkChrome from "./chrome/WorkChrome.svelte";
  import TasksCapsule from "./chrome/TasksCapsule.svelte";
  import Composer from "./composer/Composer.svelte";
  import ContextManifest from "./composer/ContextManifest.svelte";
  import AccountScopeChip from "./composer/AccountScopeChip.svelte";
  import { contextSelection } from "../lib/context-selection";
  import WorkTabPicker from "./WorkTabPicker.svelte";
  import WorkMediaPicker from "./WorkMediaPicker.svelte";
  import { mediaUrl } from "$domain/resources";
  import BrowserPane from "./pane/BrowserPane.svelte";
  import Lift from "./Lift.svelte";
  import { defaultSize } from "../lib/canvas-model";
  import { environmentPlan } from "../lib/project-environment-plan";
  import {
    environmentAgents,
    environmentItems,
    environmentLinks,
    environmentPages,
    environmentView,
  } from "../lib/project-environment";
  import { organizeExecution, pendingOrganize, elementFor } from "../lib/organize";
  import { subjectImageCandidates, subjectsOf } from "../lib/subjects";
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
  }: {
    session: WorkEnvironmentSession;
    tabs: readonly TabView[];
    spaceName: string;
    profileLabel: string;
    aiEnabled?: boolean;
    onreturn: () => void;
    /** Explicit Browse handoff for one Space tab; the pane is the default way to look at a page. */
    onopen: (id: string) => void;
    onnewtab: () => void;
    onsettings: () => void;
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
    center: (id: string) => void;
  }>();
  let selectionCount = $state(0);
  let selectedIds = $state.raw<string[]>([]);
  let accountEffect = $state.raw<WorkAccountEffectV1>({ kind: "read" });
  const contextSel = $derived(
    contextSelection(session.snapshot, selectedIds, tabs, {
      notes: context.notes,
      objectives: context.objectives,
    }),
  );
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
    const item = items.find((item) => item.id === id);
    if (!item) return;
    if (item.type === "page" && item.page) {
      openPane({ kind: "url", url: item.page.url }, id);
      return;
    }
    const reference = session.snapshot?.elements.find((element) => element.id === id)?.reference;
    if (reference?.kind === "browser") {
      if (tabs.some((tab) => tab.id === reference.tab))
        openPane({ kind: "tab", id: reference.tab }, id);
      return;
    }
    if (reference?.kind === "source") {
      if (item.source) openPane({ kind: "url", url: item.source.url }, id);
      return;
    }
    chrome?.close();
    inspected = null;
    lifted = { id, origin: canvasRef?.screenRect(id) ?? null };
  }
  // The floating browser pane: Rust owns the native hole, this owns the frame.
  let pane = $state.raw<{
    target: WorkPaneTarget;
    origin: DOMRect | null;
    phase: "opening" | "shown" | "failed";
    restore: HTMLElement | null;
  } | null>(null);
  let paneRequest: Promise<void> | null = null;
  let paneRect: WorkPaneRect | null = null;
  let paneSent: WorkPaneTarget | null = null;
  let paneDeadline: ReturnType<typeof setTimeout> | undefined;
  let paneSeen = false;
  const paneLayout = $derived(layout.workPane());
  const paneTab = $derived.by(() => {
    const current = pane;
    const id = paneLayout?.tab ?? (current?.target.kind === "tab" ? current.target.id : null);
    return id ? tabs.find((tab) => tab.id === id) : undefined;
  });
  const paneAdded = $derived(
    !!paneTab &&
      !!session.snapshot?.elements.some(
        (element) => element.reference.kind === "browser" && element.reference.tab === paneTab.id,
      ),
  );
  onMount(() => void layout.init());
  $effect(() => {
    const applied = paneLayout;
    const current = pane;
    if (applied) {
      paneSeen = true;
      clearTimeout(paneDeadline);
      if (current && current.phase === "opening") pane = { ...current, phase: "shown" };
      return;
    }
    if (paneSeen && current?.phase === "shown") {
      // Rust cleared the pane (return, tab close, scope change): drop the frame with it.
      paneSeen = false;
      paneSent = null;
      pane = null;
      current.restore?.focus({ preventScroll: true });
    }
  });
  function openPane(target: WorkPaneTarget, originId: string | null) {
    chrome?.close();
    lifted = null;
    const restore =
      pane?.restore ??
      (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    const origin = originId ? (canvasRef?.screenRect(originId) ?? null) : null;
    const rect = paneRect;
    pane = { target, origin: pane ? pane.origin : origin, phase: "opening", restore };
    if (rect && paneLayout) void requestPane(target, rect);
  }
  async function requestPane(target: WorkPaneTarget, rect: WorkPaneRect) {
    if (paneRequest || paneSent === target) return;
    paneSent = target;
    const request = (async () => {
      const okay = await workPane.show(target, rect).then(
        (result) =>
          result.outcome === "applied" ||
          result.outcome === "deferred" ||
          result.outcome === "no_op",
        () => false,
      );
      const current = pane;
      if (!current || current.target !== target) return;
      if (!okay) failPane(current);
      else {
        // Admission is not presentation: the layout projection confirms the hole.
        clearTimeout(paneDeadline);
        paneDeadline = setTimeout(() => {
          if (pane?.target === target && pane.phase === "opening") failPane(pane);
        }, 8000);
      }
    })();
    paneRequest = request;
    try {
      await request;
    } finally {
      if (paneRequest === request) paneRequest = null;
    }
  }
  function failPane(current: NonNullable<typeof pane>) {
    pane = { ...current, phase: "failed" };
    setTimeout(() => {
      if (pane?.phase === "failed") closePane();
    }, 1600);
  }
  function paneMeasured(rect: WorkPaneRect) {
    paneRect = rect;
    const current = pane;
    if (!current) return;
    if (current.phase === "opening") void requestPane(current.target, rect);
    else if (paneLayout) workPane.setRect(rect, paneLayout.generation);
  }
  function closePane() {
    const current = pane;
    pane = null;
    paneSeen = false;
    paneSent = null;
    clearTimeout(paneDeadline);
    if (paneLayout) void workPane.hide();
    current?.restore?.focus({ preventScroll: true });
  }
  function openCitation(url: string) {
    openPane({ kind: "url", url }, null);
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
  let composerFailure = $state<"account" | null>(null);
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
    snapshot
      ? environmentItems(snapshot, tabs, context.notes, context.objectives, context.media)
      : [],
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
  const agents = $derived(
    snapshot
      ? environmentAgents(snapshot, context.objectives, (objective) =>
          objectiveSession?.selected === objective
            ? objectiveSession.activity.at(-1)?.activity
            : undefined,
        )
      : { items: [], links: [], positions: {} },
  );
  const pages = $derived(
    snapshot
      ? environmentPages(snapshot, context.objectives, (objective) =>
          objectiveSession?.selected === objective ? objectiveSession.pages : [],
        )
      : { items: [], links: [], positions: {} },
  );
  const items = $derived([...results.items, ...pages.items, ...agents.items]);
  const links = $derived([
    ...scene.links,
    ...(snapshot ? environmentLinks(snapshot) : []),
    ...pages.links,
    ...agents.links,
  ]);
  const organizing = new SvelteSet<string>();
  // Planned placements render new elements where organize intends them before
  // their saved placement lands, so a canvas checkpoint never persists the
  // fallback grid over them.
  let planned = $state.raw<Record<string, { x: number; y: number; width: number; height: number }>>(
    {},
  );
  const plannedGeometry = $derived.by(() => {
    const positions: Record<string, { x: number; y: number }> = {};
    const sizes: Record<string, { width: number; height: number }> = {};
    if (!snapshot) return { positions, sizes };
    const saved = new Set(snapshot.view.placements.map((place) => place.element));
    for (const element of snapshot.elements) {
      if (saved.has(element.id)) continue;
      const place = planned[JSON.stringify(element.reference)];
      if (!place) continue;
      positions[element.id] = { x: place.x, y: place.y };
      sizes[element.id] = { width: place.width, height: place.height };
    }
    return { positions, sizes };
  });
  $effect(() => {
    const current = snapshot;
    const objectives = context.objectives;
    if (!current || session.pending || session.loading) return;
    for (const element of current.elements) {
      if (element.reference.kind !== "objective") continue;
      const projection = objectives.get(element.reference.objective);
      if (!projection) continue;
      const execution = pendingOrganize(current, projection);
      if (!execution || organizing.has(execution.id)) continue;
      organizing.add(execution.id);
      const place = current.view.placements.find((place) => place.element === element.id);
      const anchor = place ? { x: place.x, y: place.y + place.height + 48 } : { x: 80, y: 320 };
      void untrack(() =>
        organize(projection, execution, anchor).finally(() => organizing.delete(execution.id)),
      );
    }
  });
  async function organize(
    projection: WorkRuntimeProjection,
    execution: WorkExecutionFact,
    anchor: { x: number; y: number },
  ) {
    if (!session.snapshot) return;
    const plan = organizeExecution(projection, execution, anchor, session.snapshot);
    if (!plan.adds.length || !(await session.flushView())) return;
    planned = {
      ...planned,
      ...Object.fromEntries(
        plan.adds.map((add) => [JSON.stringify(add.reference), add.placement] as const),
      ),
    };
    for (const add of plan.adds) {
      if (session.snapshot && elementFor(session.snapshot, add.reference)) continue;
      if (!(await session.edit({ kind: "add", reference: add.reference, area: null }))) return;
    }
    const latest = session.snapshot;
    if (!latest) return;
    for (const relation of plan.relations) {
      const from = elementFor(latest, relation.from);
      const to = elementFor(latest, relation.to);
      if (!from || !to) continue;
      if (
        (latest.relations ?? []).some(
          (existing) =>
            existing.from === from.id && existing.to === to.id && existing.kind === relation.kind,
        )
      )
        continue;
      if (
        !(await session.edit({
          kind: "relate",
          from: from.id,
          to: to.id,
          relation: relation.kind,
        }))
      )
        return;
    }
    const placed = session.snapshot;
    if (!placed) return;
    const placements = plan.adds.flatMap((add) => {
      const element = elementFor(placed, add.reference);
      return element
        ? [
            {
              element: element.id,
              x: Math.round(add.placement.x),
              y: Math.round(add.placement.y),
              width: Math.round(add.placement.width),
              height: Math.round(add.placement.height),
            },
          ]
        : [];
    });
    const ids = new Set(placements.map((place) => place.element));
    session.checkpoint({
      ...placed.view,
      placements: [
        ...placed.view.placements.filter((place) => !ids.has(place.element)),
        ...placements,
      ],
    });
    void admitSubjectImages(projection, execution, placed);
  }
  // Subjects may name public image candidates from their cited sources. Rust
  // fetches, bounds, decodes, and stores an admitted copy; the canvas only ever
  // renders that copy. One admission per candidate URL per environment.
  const imageAdmissions = new SvelteSet<string>();
  function canonicalImageUrl(url: string): string {
    try {
      return new URL(url).toString();
    } catch {
      return url;
    }
  }
  /** The media element on this canvas that already holds one candidate. */
  function placedPicture(snapshot: WorkEnvironmentSnapshot, canonical: string): string | undefined {
    for (const element of snapshot.elements) {
      if (element.reference.kind !== "resource") continue;
      const origin = context.media.get(element.reference.resource)?.origin;
      if (origin?.kind === "fetched" && canonicalImageUrl(origin.url) === canonical)
        return element.id;
    }
    return undefined;
  }
  async function admitSubjectImages(
    projection: WorkRuntimeProjection,
    execution: WorkExecutionFact,
    placed: WorkEnvironmentSnapshot,
  ) {
    let budget = 6;
    for (const element of placed.elements) {
      if (budget <= 0) break;
      const reference = element.reference;
      if (reference.kind !== "subject" || reference.execution !== execution.id) continue;
      // A later run adds subjects beside pictures its predecessor admitted.
      const current = session.snapshot ?? placed;
      if (!current.elements.some((candidate) => candidate.id === element.id)) continue;
      const related = (current.relations ?? []).some(
        (relation) => relation.from === element.id && relation.kind === "uses",
      );
      if (related) continue;
      const run = projection.executions.find((entry) => entry.id === reference.execution);
      const artifact = run?.artifacts.find((entry) => entry.id === reference.artifact);
      const subject = artifact ? subjectsOf(artifact)[reference.index] : undefined;
      const candidate = subject && run ? subjectImageCandidates(run, subject)[0] : undefined;
      if (!candidate) continue;
      const canonical = canonicalImageUrl(candidate);
      const known = placedPicture(current, canonical);
      if (known) {
        // The picture is already here: point at it instead of fetching it twice.
        await session.edit({ kind: "relate", from: element.id, to: known, relation: "uses" });
        continue;
      }
      const key = `${placed.id} ${canonical}`;
      if (imageAdmissions.has(key)) continue;
      imageAdmissions.add(key);
      budget -= 1;
      try {
        await commands.mediaAdmitRemote(session.profile, placed.id, element.id, candidate);
      } catch {
        /* Admission is best effort; the subject keeps its honest placeholder. */
      }
    }
  }
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
            ...pages.positions,
            ...agents.positions,
            ...plannedGeometry.positions,
            ...planGeometry.positions,
            ...savedResultPositions,
            ...environmentView(snapshot).positions,
          },
          sizes: {
            ...plannedGeometry.sizes,
            ...planGeometry.sizes,
            ...savedResultSizes,
            ...environmentView(snapshot).sizes,
          },
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
  const busy = $derived(!!session.pending || session.loading || objectivePending);
  const attachedTabs = $derived(
    snapshot?.elements.flatMap((element) =>
      element.reference.kind === "browser" ? [element.reference.tab] : [],
    ) ?? [],
  );
  const loadCanvas = () => import("./WorkCanvas.svelte");
  const loadAgentLine = () => import("./AgentLine.svelte");
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
  // Leaving Work unmounts this workspace, so a return starts with no objective
  // session and the canvas loses the agent's line and its page frames. Reattach
  // the profile's session to the objective it was last on.
  $effect(() => {
    const current = snapshot;
    if (!current || objectiveSession) return;
    const objectives = current.elements.flatMap((element) =>
      element.reference.kind === "objective" ? [element.reference.objective] : [],
    );
    if (!objectives.length) return;
    untrack(() => {
      let resumed;
      try {
        resumed = workSession(session.profile);
      } catch {
        return;
      }
      const wanted = objectives.includes(resumed.selected ?? "")
        ? resumed.selected!
        : objectives.at(-1)!;
      objectiveSession = resumed;
      void resumed.start().then(() => resumed.open(wanted));
    });
  });
  async function attach(ids: string[]) {
    for (const tab of ids) {
      if (!tabs.some((candidate) => candidate.id === tab)) continue;
      if (!(await session.edit({ kind: "add", reference: { kind: "browser", tab }, area: null })))
        break;
    }
  }
  async function attachResources(ids: string[]) {
    for (const resource of ids) {
      if (
        snapshot?.elements.some(
          (e) => e.reference.kind === "resource" && e.reference.resource === resource,
        )
      )
        continue;
      if (
        !(await session.edit({
          kind: "add",
          reference: { kind: "resource", resource },
          area: null,
        }))
      )
        break;
    }
    chrome?.close();
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
  /** The work this canvas is already talking to, if its request card is here. */
  const runningObjective = $derived(
    !!objectiveSession?.projection &&
      !!snapshot?.elements.some(
        (element) =>
          element.reference.kind === "objective" &&
          element.reference.objective === objectiveSession?.selected,
      ),
  );
  /** One field, one meaning: the first message starts the work, the rest continue it. */
  async function send() {
    const text = session.composer.trim();
    if (!text || busy) return;
    const current = objectiveSession;
    if (runningObjective && current) {
      session.composer = "";
      if (activeExecution) current.enqueue(text);
      else await current.continueWith(text);
      return;
    }
    await createObjective();
  }
  async function createObjective() {
    if (!session.composer.trim() || objectivePending || busy) return;
    const current = workSession(session.profile);
    if (!current) return;
    const account = session.accountScope;
    if (
      account &&
      accountEffect.kind === "update" &&
      (!accountEffect.update.from.trim() ||
        !accountEffect.update.to.trim() ||
        accountEffect.update.from === accountEffect.update.to)
    ) {
      composerFailure = "account";
      return;
    }
    const submission = session.objectiveSubmission ?? {
      objective: session.composer.trim(),
      command: commandId(),
      attached: false,
      context: account ? null : contextSel,
      account: account ? { element: account.element, effect: accountEffect } : null,
    };
    composerFailure = null;
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
      if (submission.context && !submission.related) {
        const goal = session.snapshot?.elements.find(
          (element) =>
            element.reference.kind === "objective" && element.reference.objective === objectiveId,
        );
        for (const item of submission.context.items) {
          if (!goal || item.element === goal.id) continue;
          const known = (session.snapshot?.relations ?? []).some(
            (relation) => relation.from === goal.id && relation.to === item.element,
          );
          if (known) continue;
          if (
            !(await session.edit({
              kind: "relate",
              from: goal.id,
              to: item.element,
              relation: "uses",
            }))
          )
            return;
        }
        submission.related = true;
      }
      if (!(await current.open(objectiveId))) return;
      const basis = current.projection?.work;
      if (current.selected !== objectiveId || basis?.id !== objectiveId) return;
      objectiveOpen = false;
      session.composer = "";
      session.objectiveToAttach = null;
      session.objectiveSubmission = null;
      session.accountScope = null;
      accountEffect = { kind: "read" };
      const environmentId = session.snapshot?.id;
      if (submission.account && environmentId)
        await current.operations.begin({
          kind: "prepare_account",
          request: {
            version: 1,
            work: objectiveId,
            expected_revision: basis.revision,
            environment: environmentId,
            element: submission.account.element,
            effect: submission.account.effect,
          },
        });
      else await current.run(submission.context);
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
    onattach={(ids) => void attach(ids)}
    onopen={(id) => openPane({ kind: "tab", id }, null)}
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
{#snippet mediaPanel()}
  <WorkMediaPicker
    profile={session.profile}
    host={notesHost}
    attachedIds={snapshot?.elements.flatMap((element) =>
      element.reference.kind === "resource" ? [element.reference.resource] : [],
    ) ?? []}
    pending={busy}
    onattach={(ids) => void attachResources(ids)}
  />
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
  {#if runningObjective && objectiveSession}
    <LazyView
      loader={loadAgentLine}
      loadingLabel={m.surface_loading()}
      failureLabel={m.surface_render_failed()}
      retryLabel={m.surface_retry()}
      >{#snippet children(Line)}
        <Line
          session={objectiveSession!}
          agents={agents.items}
          draft={session.composer}
          onfocusagent={(id) => canvasRef?.center(id)}
          onsteered={() => (session.composer = "")}
          onopenpage={(tab) => {
            if (!tabs.some((candidate) => candidate.id === tab)) return;
            const origin = snapshot?.elements.find(
              (element) => element.reference.kind === "browser" && element.reference.tab === tab,
            );
            openPane({ kind: "tab", id: tab }, origin?.id ?? null);
          }}
        />{/snippet}</LazyView
    >
  {/if}
  {#if composerFailure}<p class="composer-alert" role="alert">
      {m.work_account_update_invalid()}
    </p>{/if}
{/snippet}
{#snippet composerContext()}
  {#if session.accountScope}
    <AccountScopeChip
      title={session.accountScope.title}
      origin={session.accountScope.origin}
      bind:effect={accountEffect}
      disabled={objectivePending || !!session.objectiveSubmission}
      onremove={() => {
        session.accountScope = null;
        accountEffect = { kind: "read" };
        composerFailure = null;
      }}
    />
  {:else if contextSel}
    <ContextManifest profile={session.profile} selection={contextSel} purpose="agent" />
  {/if}
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
              {links}
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
              onopenlink={openCitation}
              onselectionchange={(ids: string[]) => {
                const owned = ids.filter((id) => authoritative.has(id));
                selectionCount = owned.length;
                selectedIds = owned;
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
                if (action === "choose" || action === "unchoose") {
                  void session.edit(
                    action === "choose"
                      ? { kind: "decide", element: id, choice: m.work_env_chosen() }
                      : { kind: "undecide", element: id },
                  );
                  return;
                }
                if (action?.startsWith("area:")) {
                  void session.edit({
                    kind: "assign_area",
                    element: id,
                    area: action.slice(5) || null,
                  });
                  return;
                }
                if (action === "ask") {
                  composerElement?.querySelector<HTMLElement>("textarea")?.focus();
                  return;
                }
                if (action === "account") {
                  const item = items.find((item) => item.id === id);
                  if (item?.type === "tab" && !item.unavailable && item.detail) {
                    session.accountScope = { element: id, title: item.title, origin: item.detail };
                    accountEffect = { kind: "read" };
                    composerFailure = null;
                    composerElement?.querySelector<HTMLElement>("textarea")?.focus();
                  }
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
                onopen={openCitation}
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
                onopencitation={openCitation}
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
      {#if liftedElement?.reference.kind === "resource" && liftedItem?.media}
        {@const image =
          liftedItem.media.asset.kind === "image"
            ? mediaUrl(liftedItem.media.profile, liftedItem.media.asset.digest)
            : null}
        <div class="lift-media">
          {#if image}<img src={image} alt={liftedItem.title} />{:else}
            <p>{liftedItem.media.asset.mime}</p>
            <Button
              size="compact"
              onclick={() => {
                const id =
                  liftedElement?.reference.kind === "resource"
                    ? liftedElement.reference.resource
                    : null;
                if (id) void commands.mediaOpen(session.profile, id);
              }}>{m.work_media_open_file()}</Button
            >
          {/if}
        </div>
      {:else if liftedElement?.reference.kind === "resource"}
        <LazyView
          loader={loadNoteEditorHost}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(Host)}<Host
              profile={session.profile}
              host={notesHost}
              onlink={openCitation}
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
              onopencitation={openCitation}
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
                  onopen={openCitation}
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
                const reference = liftedElement?.reference;
                const id = liftedElement?.id ?? null;
                lifted = null;
                if (reference?.kind === "browser" && tabs.some((tab) => tab.id === reference.tab))
                  openPane({ kind: "tab", id: reference.tab }, id);
              }}>{m.work_env_open_here()}</Button
            >{/if}
        </div>
      {/if}
    </Lift>
  {/if}
  {#if pane && cardBounds}
    <BrowserPane
      tab={paneTab}
      applied={paneLayout}
      bounds={cardBounds}
      origin={pane.origin}
      phase={pane.phase}
      added={paneAdded}
      onmeasure={paneMeasured}
      onnavigate={(input) => {
        if (paneTab) void commands.tabsNavigate(paneTab.id, input);
      }}
      onback={() => {
        if (paneTab) void commands.tabsBack(paneTab.id);
      }}
      onforward={() => {
        if (paneTab) void commands.tabsForward(paneTab.id);
      }}
      onreload={() => {
        if (paneTab) void commands.tabsReload(paneTab.id);
      }}
      onopenbrowse={() => {
        if (paneTab) onopen(paneTab.id);
      }}
      onadd={() => {
        if (paneTab) void attach([paneTab.id]);
      }}
      onclose={closePane}
    />
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
      media: mediaPanel,
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
        placeholder={runningObjective ? m.work_composer_continue() : m.work_composer_start()}
        disabled={objectivePending || !!session.objectiveSubmission}
        {busy}
        above={composerAbove}
        context={contextSel || session.accountScope ? composerContext : undefined}
        onsubmit={() => void send()}
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

  .lift-media {
    display: grid;
    place-items: center;
    gap: 12px;
    block-size: 100%;
    min-block-size: 0;
  }

  .lift-media img {
    max-inline-size: 100%;
    max-block-size: 100%;
    object-fit: contain;
    border-radius: var(--radius-sm);
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
</style>
