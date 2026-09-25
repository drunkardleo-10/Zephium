<script lang="ts">
  import { untrack, onMount } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import { WorkEnvironmentContext, type WorkEnvironmentSession } from "$domain/work-environment";
  import { commandId, workSession, type WorkSession } from "$domain/work";
  import { taskSession } from "$domain/resources";
  import { noteSession } from "$domain/notes";
  import type {
    TabView,
    WorkHumanAccountV1,
    WorkHumanPageIdV1,
    WorkHumanRegionV1,
    WorkEnvironmentReference,
    WorkFileEvidenceV1,
    WorkAccountEffectV1,
    WorkEnvironmentSnapshot,
    WorkExecutionFact,
    WorkRuntimeProjection,
  } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import { layout } from "$domain/layout";
  import { workPane, type WorkPaneRect, type WorkPaneTarget } from "$domain/work-pane";
  import { WorkHumanSession, type WorkHumanFailure } from "$domain/work-human";
  import { preferences } from "$domain/preferences";
  import { loadNotes, loadNoteHost } from "$features/notes";
  import { loadTasks } from "$features/tasks";
  import { IS_MAC } from "$shared/platform";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import LazyView from "$shared/ui/LazyView";
  import {
    Archive01Icon,
    ArrowLeft02Icon,
    ChartColumnIcon,
    ComputerTerminal01Icon,
    File01Icon,
    FolderAddIcon,
    GlobalIcon,
    Image01Icon,
    LayoutGridIcon,
    Link04Icon,
    MapsIcon,
    NoteAddIcon,
    Pdf01Icon,
    Search01Icon,
    Settings02Icon,
    Table01Icon,
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
  import { mediaSize, mediaUrl } from "$domain/resources";
  import BrowserPane from "./pane/BrowserPane.svelte";
  import Lift from "./Lift.svelte";
  import LiftHeader from "./LiftHeader.svelte";
  import { commandRecord, pageGroups } from "../lib/project-environment-stage";
  import HostGlyph from "./cards/HostGlyph.svelte";
  import { clipText, defaultSize } from "../lib/canvas-model";
  import { homePath } from "../lib/work-files";
  import { youtubeThumbnail } from "../lib/link-media";
  import { stepResult } from "../lib/plan-steps";
  import { isAgentExecution, isLive } from "../lib/agent-steps";
  import { environmentPlan } from "../lib/project-environment-plan";
  import {
    environmentAgents,
    environmentClusters,
    environmentFiles,
    environmentItems,
    environmentLinks,
    environmentPages,
    environmentPictures,
    environmentSources,
    environmentSteps,
    environmentView,
    fileEvidence,
    viewPlacements,
  } from "../lib/project-environment";
  import {
    environmentRequests,
    environmentStages,
    type WorkStage,
  } from "../lib/project-environment-thread";
  import { failureLine, humanPage, regionOf, sameRegion } from "../lib/work-human";
  import { openOver, type PaneRect } from "../lib/pane-geometry";
  import { organizeExecution, pendingOrganize, elementFor } from "../lib/organize";
  import { subjectImageCandidates, subjectsOf } from "../lib/subjects";
  import { environmentResults, type ResultReference } from "../lib/project-environment-results";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import type { CanvasView, CanvasItem, CanvasPosition } from "../lib/canvas-model";
  import type { WorkEnvironmentPanel } from "../lib/work-environment";
  import * as m from "$shared/i18n/messages";
  let {
    session,
    tabs,
    spaceName,
    profileLabel,
    currentTabId = null,
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
    /** The Space's current tab, so the picker reads like its tab strip. */
    currentTabId?: string | null;
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
  // The pages the runs on this canvas are holding open for a person.
  const human = untrack(() => new WorkHumanSession(session.profile));
  onMount(() => {
    void human.start();
    return () => human.dispose();
  });
  $effect(() => {
    human.update([...context.objectives.keys()]);
  });
  onMount(() => {
    session.tabsIntroduced = true;
  });
  const notesHost = $derived(`environment:${session.space}`);
  const notes = untrack(() => noteSession(session.profile, notesHost));
  onMount(() => {
    void notes?.start();
    return () => notes?.stop();
  });
  const taskList = untrack(() => taskSession(session.profile, `environment:${session.space}`));
  onMount(() => {
    void taskList.start();
    return () => taskList.stop();
  });
  let objectiveSession = $state.raw<WorkSession | null>(null);
  let inspected = $state<string | null>(null);
  let lifted = $state.raw<{
    id: string;
    origin: DOMRect | null;
    /** A change the run proposed: the lift opens on the step, not on a card. */
    proposal?: string;
  } | null>(null);
  let canvasRef = $state<{
    screenRect: (id: string) => DOMRect | null;
    flowPosition: (clientX: number, clientY: number) => CanvasPosition | null;
    selectionBounds: (only?: readonly string[]) => {
      x: number;
      y: number;
      width: number;
      height: number;
      ids: string[];
    } | null;
    placeArea: (
      area: string,
      rect: { x: number; y: number; width: number; height: number },
    ) => void;
    center: (id: string) => void;
    focusCard: (id: string) => void;
  }>();
  let selectionCount = $state(0);
  let selectedIds = $state.raw<string[]>([]);
  let accountEffect = $state.raw<WorkAccountEffectV1>({ kind: "read" });
  const contextSel = $derived(
    contextSelection(session.snapshot, selectedIds, tabs, {
      notes: context.notes,
      objectives: context.objectives,
      media: context.mediaRevisions,
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
    // A step is part of its result: it opens the result whole.
    const owner = item.type === "step" ? stepResult(id) : null;
    if (owner) {
      openLift(owner);
      return;
    }
    if (results.references.has(id)) {
      void openResult(id);
      return;
    }
    if (item.type === "page" && item.page) {
      // A page the run is waiting on opens as a takeover, not as a copy in the
      // person's own profile: a sign-in there never reaches the agent.
      if (item.page.human?.phase === "waiting_for_human") openTakeover(id);
      else openPane({ kind: "url", url: item.page.url }, id);
      return;
    }
    const reference = session.snapshot?.elements.find((element) => element.id === id)?.reference;
    if (reference?.kind === "link") {
      if (youtubeThumbnail(reference.url)) playHere(id);
      else openPane({ kind: "url", url: reference.url }, id);
      return;
    }
    if (reference?.kind === "browser") {
      if (tabs.some((tab) => tab.id === reference.tab))
        openPane({ kind: "tab", id: reference.tab }, id);
      return;
    }
    lift(id);
  }
  /** A lifted tab card's one action: the tab opens in the pane, over its card. */
  function openLiftedTab() {
    const element = liftedElement;
    const reference = element?.reference;
    lifted = null;
    if (reference?.kind === "browser" && tabs.some((tab) => tab.id === reference.tab))
      openPane({ kind: "tab", id: reference.tab }, element!.id);
  }
  /** A video plays in the pane, opened over its card and at least the pane's minimum. */
  function playHere(id: string) {
    const reference = snapshot?.elements.find((element) => element.id === id)?.reference;
    if (reference?.kind !== "link") return;
    const card = canvasRef?.screenRect(id);
    if (card && !pane) openOver(card);
    openPane({ kind: "url", url: reference.url }, id);
  }
  function lift(id: string) {
    chrome?.close();
    inspected = null;
    lifted = { id, origin: canvasRef?.screenRect(id) ?? null };
    if (snapshot?.elements.find((element) => element.id === id)?.reference.kind === "subject")
      void admitGallery(id);
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
    endTakeover(true);
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
  // The takeover: Rust presents the agent's own view inside the well this pane
  // reserves. There is no resize command, so a moved well is released and
  // presented again.
  let takeover = $state.raw<{
    work: string;
    card: string;
    page: WorkHumanPageIdV1;
    host: string;
    url: string;
    restore: HTMLElement | null;
  } | null>(null);
  let takeoverError = $state<string | null>(null);
  let takeoverSent: WorkHumanRegionV1 | null = null;
  let takeoverSeen = false;
  let takeoverQueue: Promise<unknown> = Promise.resolve();
  const takeoverPage = $derived.by(() => {
    const current = takeover;
    if (!current) return null;
    const found = (human.pages.get(current.work) ?? []).find(
      (candidate) =>
        candidate.id.attempt === current.page.attempt &&
        candidate.id.step === current.page.step &&
        candidate.id.generation === current.page.generation,
    );
    return found ? humanPage(found) : null;
  });
  const takeoverView = $derived.by(() => {
    const current = takeover;
    const page = takeoverPage;
    const bounds = cardBounds;
    return current && page && bounds ? { current, page, bounds } : null;
  });
  /** One command at a time: a release and its re-presentation never interleave. */
  function applyTakeover(job: () => Promise<WorkHumanFailure | null>, report: boolean) {
    const next = takeoverQueue.then(async () => {
      const failure = await job();
      if (report) takeoverError = failure ? failureLine(failure) : null;
    });
    takeoverQueue = next.catch(() => undefined);
  }
  function openTakeover(card: string) {
    const item = items.find((entry) => entry.id === card);
    const state = item?.page?.human;
    if (!state || state.phase !== "waiting_for_human") return;
    const work = [...human.pages].find(([, pages]) =>
      pages.some(
        (candidate) =>
          candidate.id.attempt === state.attempt &&
          candidate.id.step === state.step &&
          candidate.id.generation === state.generation,
      ),
    )?.[0];
    if (!work) return;
    chrome?.close();
    lifted = null;
    inspected = null;
    closePane();
    const restore =
      takeover?.restore ??
      (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    takeoverSeen = false;
    takeoverSent = null;
    takeoverError = null;
    takeover = {
      work,
      card,
      page: { attempt: state.attempt, step: state.step, generation: state.generation },
      host: item?.page?.host ?? "",
      url: item?.page?.url ?? "",
      restore,
    };
    canvasRef?.center(card);
  }
  function endTakeover(release: boolean) {
    const current = takeover;
    takeover = null;
    takeoverError = null;
    takeoverSeen = false;
    takeoverSent = null;
    if (current && release) applyTakeover(() => human.release(current.work, current.page), false);
    current?.restore?.focus({ preventScroll: true });
  }
  // Once the page is the agent's again the pane has nothing left to hold.
  $effect(() => {
    if (!takeover) return;
    const page = takeoverPage;
    if (page && page.phase !== "reading" && page.phase !== "released") {
      takeoverSeen = true;
      return;
    }
    // A settled phase means the agent already has it; a page that vanished
    // from the projection is unknown, so its view is released to be sure.
    if (page) untrack(() => endTakeover(false));
    else if (takeoverSeen) untrack(() => endTakeover(true));
  });
  function takeoverRegion(box: PaneRect | null) {
    const current = takeover;
    if (!current) return;
    const next = box
      ? regionOf(box, { width: window.innerWidth, height: window.innerHeight })
      : null;
    // A well Rust would refuse says so rather than waiting on a view forever.
    if (box && !next) takeoverError = failureLine("invalid");
    if (sameRegion(next, takeoverSent)) return;
    const sent = takeoverSent;
    takeoverSent = next;
    if (sent) applyTakeover(() => human.release(current.work, current.page), false);
    if (next) applyTakeover(() => human.present(current.work, current.page, next), true);
  }
  function continueTakeover(account: WorkHumanAccountV1) {
    const current = takeover;
    if (!current) return;
    takeoverSent = null;
    applyTakeover(() => human.continue(current.work, current.page, account), true);
  }
  /** A window that goes away hands the page back; the command is already sent. */
  function abandonTakeover() {
    const current = takeover;
    if (current) void human.release(current.work, current.page);
  }
  onMount(() => abandonTakeover);
  function liftSize(item: CanvasItem | undefined) {
    if (!item) return { width: 720, height: 520 };
    // A product compare wants every column at once, not a scrollbar.
    if (item.artifact?.content.kind === "matrix")
      return {
        width: Math.min(1180, 320 + item.artifact.content.subjects.length * 200),
        height: 640,
      };
    switch (item.type) {
      case "note":
        return { width: 760, height: 620 };
      case "tab":
        return { width: 520, height: 260 };
      case "objective":
        return { width: 640, height: 560 };
      case "responsibility":
        return { width: 480, height: 320 };
      case "sources":
        return { width: 560, height: 560 };
      case "subject":
        return { width: 860, height: 620 };
      default: {
        const base = defaultSize(item);
        return { width: Math.max(720, base.width + 200), height: Math.max(520, base.height + 160) };
      }
    }
  }
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
  let mediaKind = $state<"document" | "image" | "link" | "folder">("document");
  let linkDraft = $state("");
  let linkPending = $state(false);
  let linkFailure = $state(false);
  let folderDraft = $state("");
  let folderPending = $state(false);
  let folderRefused = $state(false);
  let folderNotice: ReturnType<typeof setTimeout> | undefined;
  /** One quiet line, and it goes away on its own. */
  function refuseFolder() {
    folderRefused = true;
    clearTimeout(folderNotice);
    folderNotice = setTimeout(() => (folderRefused = false), 6000);
  }
  /** A folder becomes a card only after the application admits its path. A file
   * among dropped paths is not a refusal a person needs to hear about. */
  async function addFolder(path: string, at?: CanvasPosition | null, dropped = false) {
    if (folderPending || busy) return false;
    folderPending = true;
    try {
      const admitted = await commands.workAdmitFolder(session.profile, path).catch(() => null);
      if (admitted?.status !== "ok" || admitted.data.kind !== "admitted") {
        const file =
          admitted?.status === "ok" &&
          admitted.data.kind === "refused" &&
          admitted.data.not_a_folder;
        if (!(dropped && file)) refuseFolder();
        return false;
      }
      return await placeFolder(admitted.data, at ?? null);
    } finally {
      folderPending = false;
    }
  }
  function placeFolder(folder: { path: string; name: string }, at: CanvasPosition | null) {
    return place({ kind: "folder", path: folder.path, name: folder.name }, "folder", at);
  }
  /** The native folder picker; a cancelled choice says nothing. */
  async function chooseFolder() {
    if (folderPending || busy) return;
    folderPending = true;
    try {
      const chosen = await commands.workPickFolder(session.profile).catch(() => null);
      if (chosen?.status !== "ok" || !chosen.data) return;
      if (chosen.data.kind !== "admitted") {
        refuseFolder();
        return;
      }
      if (await placeFolder(chosen.data, null)) chrome?.close();
    } finally {
      folderPending = false;
    }
  }
  /** Shows an admitted folder, or a file inside one, where it lives. */
  function reveal(path: string) {
    void commands.workRevealPath(session.profile, path).catch(() => null);
  }
  /** Adds an element and stands it where the person put it, or in the middle. */
  async function place(
    reference: WorkEnvironmentReference,
    type: "folder" | "link",
    at: CanvasPosition | null,
  ) {
    if (session.snapshot && elementFor(session.snapshot, reference)) return true;
    if (!(await session.flushView())) return false;
    if (!(await session.edit({ kind: "add", reference, area: null }))) return false;
    const current = session.snapshot;
    const element = current ? elementFor(current, reference) : undefined;
    const point = at ?? canvasCentre();
    if (!current || !element || !point) return true;
    const size = defaultSize({ id: element.id, title: "", kind: "", detail: "", status: "", type });
    session.checkpoint({
      ...current.view,
      placements: [
        ...current.view.placements.filter((place) => place.element !== element.id),
        {
          element: element.id,
          x: Math.round(point.x - size.width / 2),
          y: Math.round(point.y - size.height / 2),
          ...size,
        },
      ],
    });
    return true;
  }
  function canvasCentre(): CanvasPosition | null {
    const bounds = cardBounds;
    return bounds
      ? (canvasRef?.flowPosition(bounds.left + bounds.width / 2, bounds.top + bounds.height / 2) ??
          null)
      : null;
  }
  // Finder drops: the application hands the frame the dropped paths and the drop
  // point in CSS pixels. Granted folders land where they were dropped; a file
  // among them is simply not a folder, and says nothing.
  onMount(() => {
    const dropped = (event: Event) => {
      const detail = (event as CustomEvent<{ paths?: unknown; x?: unknown; y?: unknown }>).detail;
      const paths = Array.isArray(detail?.paths)
        ? detail.paths.filter((path): path is string => typeof path === "string")
        : [];
      if (!paths.length) return;
      const at =
        typeof detail?.x === "number" && typeof detail?.y === "number"
          ? canvasRef?.flowPosition(detail.x, detail.y)
          : null;
      void dropFolders(paths.slice(0, 8), at ?? null);
    };
    window.addEventListener("zephium:work-paths-dropped", dropped);
    return () => {
      window.removeEventListener("zephium:work-paths-dropped", dropped);
      clearTimeout(folderNotice);
    };
  });
  async function dropFolders(paths: readonly string[], at: CanvasPosition | null) {
    let index = 0;
    for (const path of paths) {
      const placed = await addFolder(
        path,
        at ? { x: at.x + index * 24, y: at.y + index * 24 } : null,
        true,
      );
      if (placed) index += 1;
    }
  }
  /** Only an explicit https or http address; anything else is not a link. */
  function linkUrl(raw: string): string | null {
    const text = raw.trim();
    if (!text) return null;
    try {
      const url = new URL(/^[a-z][a-z0-9+.-]*:/iu.test(text) ? text : `https://${text}`);
      return url.protocol === "https:" || url.protocol === "http:" ? url.toString() : null;
    } catch {
      return null;
    }
  }
  /** A pasted link becomes a card of its own; it opens in the pane, like a source. */
  async function addLink() {
    const url = linkUrl(linkDraft);
    if (!url || linkPending || busy) return;
    linkPending = true;
    linkFailure = false;
    try {
      const title = host(url);
      if (!(await place({ kind: "link", url, title }, "link", null))) {
        linkFailure = true;
        return;
      }
      linkDraft = "";
      chrome?.close();
    } finally {
      linkPending = false;
    }
  }
  function host(url: string): string {
    try {
      return new URL(url).host.replace(/^www\./u, "");
    } catch {
      return "";
    }
  }
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
  /** What the run this canvas is watching is doing right now. */
  const signalOf = (objective: string) =>
    objectiveSession?.selected === objective
      ? objectiveSession.activity.at(-1)?.activity
      : undefined;
  const pictures = $derived(
    snapshot ? environmentPictures(snapshot, context.objectives, context.media) : new Map(),
  );
  /** The pages each work recorded: the live session has the newest frames for
   * the work it is on; every other work keeps the frames the context read. */
  const recordedPages = (objective: string) =>
    objectiveSession?.selected === objective && objectiveSession.pages.length
      ? objectiveSession.pages
      : (context.pages.get(objective) ?? []);
  /** Every message of the thread, in order, with the card its run hangs from. */
  const stages = $derived(
    snapshot ? environmentStages(snapshot, context.objectives, recordedPages) : [],
  );
  const agents = $derived(
    snapshot
      ? environmentAgents(snapshot, context.objectives, signalOf, stages)
      : { items: [], links: [], positions: {} },
  );
  const sources = $derived(
    snapshot
      ? environmentSources(snapshot, context.objectives, stages)
      : { items: [], links: [], positions: {} },
  );
  const pages = $derived(
    snapshot
      ? environmentPages(
          snapshot,
          context.objectives,
          stages,
          recordedPages,
          signalOf,
          new Set(agents.items.map((item) => item.id)),
          (objective) => human.pages.get(objective) ?? [],
        )
      : { items: [], links: [], positions: {} },
  );
  /** The page this canvas's run is held on, so the line can point at its card. */
  const agentWaiting = $derived.by(() => {
    const work = objectiveSession?.projection?.work.id;
    const opened = work ? (human.pages.get(work) ?? []) : [];
    if (!opened.length) return null;
    for (const item of pages.items) {
      const state = item.page?.human;
      if (state?.phase !== "waiting_for_human") continue;
      if (
        !opened.some(
          (candidate) =>
            candidate.id.attempt === state.attempt &&
            candidate.id.step === state.step &&
            candidate.id.generation === state.generation,
        )
      )
        continue;
      return {
        card: item.id,
        host: item.page?.host ?? "",
        reason: state.reason,
        remaining: state.remaining,
      };
    }
    return null;
  });
  const requests = $derived(environmentRequests(stages));
  const steps = $derived(
    snapshot
      ? environmentSteps(snapshot, context.objectives, stages)
      : { items: [], links: [], positions: {} },
  );
  const files = $derived(
    snapshot ? environmentFiles(context.objectives, stages) : { items: [], positions: {} },
  );
  const clusters = $derived(environmentClusters(stages));
  const items = $derived([
    ...results.items,
    ...requests.items,
    ...sources.items,
    ...pages.items,
    ...files.items,
    ...steps.items,
    ...agents.items,
  ]);
  const links = $derived([
    ...scene.links,
    ...(snapshot ? environmentLinks(snapshot) : []),
    ...clusters.links,
    ...requests.links,
    ...sources.links,
    ...pages.links,
    ...steps.links,
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
      const stage = stages.find((stage) => stage.executions.includes(execution.id));
      const place =
        stage?.place ?? current.view.placements.find((place) => place.element === element.id);
      // What a run places lands in its stage's clusters, beside its request.
      const anchor = place ? { x: place.x, y: place.y } : { x: 80, y: 120 };
      void untrack(() =>
        organize(projection, execution, anchor, stage).finally(() =>
          organizing.delete(execution.id),
        ),
      );
    }
  });
  async function organize(
    projection: WorkRuntimeProjection,
    execution: WorkExecutionFact,
    anchor: { x: number; y: number },
    stage: WorkStage | undefined,
  ) {
    if (!session.snapshot) return;
    const plan = organizeExecution(projection, execution, anchor, session.snapshot, stage);
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
    void pictureSubjects();
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
  /** A subject shows a picture once any "uses" relation leaves it. */
  function pictured(current: WorkEnvironmentSnapshot, element: string): boolean {
    return (current.relations ?? []).some(
      (relation) => relation.from === element && relation.kind === "uses",
    );
  }
  /** Subjects and video links on this canvas still without a picture, with their candidates in order. */
  function unpictured(current: WorkEnvironmentSnapshot) {
    return current.elements.flatMap((element) => {
      const reference = element.reference;
      if (reference.kind === "link") {
        // A video link's picture is its thumbnail, admitted like a subject's.
        const thumbnail = youtubeThumbnail(reference.url);
        if (!thumbnail || pictured(current, element.id)) return [];
        if (admittedFor.has(`${current.id} ${element.id}`)) return [];
        return [{ element: element.id, candidates: [thumbnail] }];
      }
      if (reference.kind !== "subject" || pictured(current, element.id)) return [];
      if (admittedFor.has(`${current.id} ${element.id}`)) return [];
      const run = context.objectives
        .get(reference.objective)
        ?.executions.find((entry) => entry.id === reference.execution);
      const artifact = run?.artifacts.find((entry) => entry.id === reference.artifact);
      const subject = artifact ? subjectsOf(artifact)[reference.index] : undefined;
      const candidates = subject && run ? subjectImageCandidates(run, subject) : [];
      return candidates.length ? [{ element: element.id, candidates }] : [];
    });
  }
  // One queue per canvas, not a per-pass budget: it runs until every pictured
  // subject has one picture, falls back to the next candidate when Rust
  // refuses one, and retries a refused candidate once after a pause.
  const PICTURE_RETRY_MS = 5000;
  const refusals = new SvelteMap<string, { count: number; after: number }>();
  const admittedFor = new SvelteSet<string>();
  let picturing = false;
  let pictureAgain = false;
  let pictureTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const current = snapshot;
    // A projection update can bring candidates for subjects already placed.
    const runs = [...context.objectives.values()];
    const links = current?.elements.some((element) => element.reference.kind === "link");
    if (!current || (!runs.length && !links) || session.pending || session.loading) return;
    untrack(() => void pictureSubjects());
  });
  $effect(() => () => clearTimeout(pictureTimer));
  async function admitPicture(environment: string, element: string, candidate: string) {
    try {
      const result = await commands.mediaAdmitRemote(
        session.profile,
        environment,
        element,
        candidate,
      );
      return result.status === "ok" && result.data.kind === "admitted";
    } catch {
      return false;
    }
  }
  async function pictureSubjects() {
    if (picturing) {
      pictureAgain = true;
      return;
    }
    picturing = true;
    try {
      do {
        pictureAgain = false;
        const current = session.snapshot;
        if (!current) break;
        let retry = Infinity;
        for (const { element, candidates } of unpictured(current)) {
          for (const candidate of candidates) {
            const latest = session.snapshot ?? current;
            if (latest.id !== current.id || pictured(latest, element)) break;
            const canonical = canonicalImageUrl(candidate);
            const known = placedPicture(latest, canonical);
            if (known) {
              // The picture is already here: point at it instead of fetching it twice.
              if (
                await session.edit({ kind: "relate", from: element, to: known, relation: "uses" })
              )
                break;
              continue;
            }
            const key = `${current.id} ${canonical}`;
            const refused = refusals.get(key) ?? { count: 0, after: 0 };
            if (refused.count >= 2) continue;
            if (Date.now() < refused.after) {
              retry = Math.min(retry, refused.after);
              continue;
            }
            imageAdmissions.add(key);
            if (await admitPicture(current.id, element, candidate)) {
              admittedFor.add(`${current.id} ${element}`);
              break;
            }
            const after = Date.now() + PICTURE_RETRY_MS;
            refusals.set(key, { count: refused.count + 1, after });
            if (!refused.count) retry = Math.min(retry, after);
          }
        }
        if (retry !== Infinity) {
          clearTimeout(pictureTimer);
          pictureTimer = setTimeout(() => void pictureSubjects(), Math.max(0, retry - Date.now()));
        }
      } while (pictureAgain);
    } finally {
      picturing = false;
    }
  }
  const liftedItem = $derived(items.find((item) => item.id === lifted?.id));
  const liftedCommand = $derived(
    liftedItem?.command?.record
      ? commandRecord(context.objectives, liftedItem.command.record)
      : undefined,
  );
  const liftedElement = $derived(snapshot?.elements.find((element) => element.id === lifted?.id));
  /** Every admitted picture of one subject element, in the order it admitted them. */
  function picturesOf(element: string) {
    const current = snapshot;
    if (!current) return [];
    const resources = new Map(
      current.elements.flatMap((entry) =>
        entry.reference.kind === "resource" ? [[entry.id, entry.reference.resource]] : [],
      ),
    );
    return (current.relations ?? []).flatMap((relation) => {
      if (relation.kind !== "uses" || relation.from !== element) return [];
      const resource = resources.get(relation.to);
      const asset = resource ? context.media.get(resource) : undefined;
      return asset?.kind === "image"
        ? [{ profile: current.profile, digest: asset.digest, name: asset.name }]
        : [];
    });
  }
  const liftedPictures = $derived(
    liftedElement?.reference.kind === "subject" ? picturesOf(liftedElement.id) : [],
  );
  /** Pages past the cluster's cap have no card; their stage's Sources lift lists them. */
  const foldedPages = $derived.by(() => {
    const item = liftedItem;
    if (item?.type !== "sources") return [];
    const run = item.id.split(":").at(-1) ?? "";
    const stage = stages.find((entry) => entry.executions.includes(run));
    const projection = stage ? context.objectives.get(stage.objective) : undefined;
    if (!stage || !projection) return [];
    const shown = new Set(pages.items.map((page) => page.id));
    return stage.executions
      .flatMap((id) => {
        const execution = projection.executions.find((entry) => entry.id === id);
        return execution && isAgentExecution(execution)
          ? pageGroups(execution, recordedPages(stage.objective))
          : [];
      })
      .filter((group) => !shown.has(group.id));
  });
  /** The one meta line of a media lift: what it is, how large, where it came from. */
  function mediaMeta(asset: NonNullable<CanvasItem["media"]>["asset"]) {
    const origin = asset.origin.kind === "fetched" ? host(asset.origin.url) : "";
    return [asset.mime, mediaSize(asset.bytes), origin].filter(Boolean).join(" · ");
  }
  // Opening a product view is an explicit act: it is worth admitting the other
  // pictures the run observed for that subject, and only then.
  const gallery = new SvelteSet<string>();
  async function admitGallery(element: string) {
    const current = snapshot;
    const reference = current?.elements.find((entry) => entry.id === element)?.reference;
    if (!current || reference?.kind !== "subject" || gallery.has(element)) return;
    gallery.add(element);
    const run = context.objectives
      .get(reference.objective)
      ?.executions.find((execution) => execution.id === reference.execution);
    const artifact = run?.artifacts.find((artifact) => artifact.id === reference.artifact);
    const subject = artifact ? subjectsOf(artifact)[reference.index] : undefined;
    if (!run || !subject) return;
    const known = picturesOf(element).flatMap((picture) => {
      const origin = [...context.media.values()].find(
        (asset) => asset.digest === picture.digest,
      )?.origin;
      return origin?.kind === "fetched" ? [canonicalImageUrl(origin.url)] : [];
    });
    for (const candidate of subjectImageCandidates(run, subject).slice(0, 3)) {
      if (known.length >= 3) break;
      const canonical = canonicalImageUrl(candidate);
      if (known.includes(canonical)) continue;
      known.push(canonical);
      const key = `${current.id} ${canonical}`;
      if (imageAdmissions.has(key)) continue;
      imageAdmissions.add(key);
      try {
        await commands.mediaAdmitRemote(session.profile, current.id, element, candidate);
      } catch {
        /* The other pictures are a nicety; the first one already stands. */
      }
    }
  }

  const authoritative = $derived(new Set(snapshot?.elements.map((element) => element.id) ?? []));
  let liftSource = $state.raw<EvidenceReference | null>(null);
  const loadResult = () => import("./WorkResultInspector.svelte");
  const loadFile = () => import("./WorkFileInspector.svelte");
  const loadSubject = () => import("./WorkSubjectInspector.svelte");
  /** The file a source row opened, shown as the run recorded it. */
  let liftFile = $state.raw<WorkFileEvidenceV1 | null>(null);
  /** One surface: a result opens in the lift, on the run that produced it. */
  async function openResult(id: string, source: EvidenceReference | null = null) {
    const reference = results.references.get(id);
    if (!reference) return;
    const current = workSession(session.profile);
    if (!current) return;
    objectiveSession = current;
    await current.start();
    if (!(await current.open(reference.objective))) return;
    notesOpen = false;
    liftSource = source;
    lift(id);
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
            ...requests.positions,
            ...sources.positions,
            ...pages.positions,
            ...files.positions,
            ...steps.positions,
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
  /** Every folder on this canvas is granted to the runs it starts. */
  const grantedFolders = $derived(
    snapshot?.elements.flatMap((element) =>
      element.reference.kind === "folder" ? [element.reference.path] : [],
    ) ?? [],
  );
  $effect(() => {
    const current = objectiveSession;
    const folders = grantedFolders;
    if (current) current.folders = folders;
  });
  const busy = $derived(!!session.pending || session.loading || objectivePending);
  const attachedTabs = $derived(
    snapshot?.elements.flatMap((element) =>
      element.reference.kind === "browser" ? [element.reference.tab] : [],
    ) ?? [],
  );
  const loadCanvas = () => import("./WorkCanvas.svelte");
  const loadAgentLine = () => import("./AgentLine.svelte");
  const loadTakeover = () => import("./pane/TakeoverPane.svelte");
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
      !current.note?.id ||
      environmentId !== session.snapshot?.id
    )
      return;
    await session.edit({
      kind: "add",
      reference: { kind: "resource", resource: current.note.id },
      area: null,
    });
  }
  async function createNote() {
    const current = notes;
    if (!current) return;
    await current.start();
    // A note has no file, and so no identity to attach, until it has text.
    await current.create(`# ${m.note_untitled()}\n`);
    if (!current.note?.id) return;
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
      placements: viewPlacements(snapshot, view, stages),
    };
    if (
      JSON.stringify({ ...next, revision: "" }) !==
      JSON.stringify({ ...(session.viewDraft?.view ?? snapshot.view), revision: "" })
    )
      session.checkpoint(next);
  }
  async function closeNotes() {
    if (!notes || (await notes.flush())) notesOpen = false;
  }
  const taskItems = $derived(taskList.rows);
  const tasksDone = $derived(taskItems.filter((task) => task.status === "done").length);
  const activeExecution = $derived.by(() => {
    const projection = objectiveSession?.projection;
    return !!projection?.executions.some((execution) => isLive(projection, execution));
  });
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
  /** One click makes an Area: around the selection when there is one. */
  async function createArea(title: string = m.work_env_area()) {
    const name = title.trim() || m.work_env_area();
    if (selectionCount > 0) {
      await groupSelection(name);
      return;
    }
    if (await session.edit({ kind: "create_area", title: name })) {
      areaTitle = "";
      chrome?.close();
    }
  }
  const ownedElements = (ids: readonly string[]) =>
    ids.filter((id) => snapshot?.elements.some((element) => element.id === id));
  /** A new area around the person's selected elements; the canvas places it before they move in. */
  async function groupSelection(title: string, ids: readonly string[] = selectedIds) {
    const bounds = canvasRef?.selectionBounds(ownedElements(ids));
    const current = snapshot;
    if (!bounds || !current || !title.trim()) return;
    if (!(await session.flushView())) return;
    if (!(await session.edit({ kind: "create_area", title: title.trim() }))) return;
    const created = session.snapshot?.areas.find(
      (area) => !current.areas.some((known) => known.id === area.id),
    );
    if (!created) return;
    const { ids: members, ...rect } = bounds;
    canvasRef?.placeArea(created.id, rect);
    for (const id of members)
      if (!(await session.edit({ kind: "assign_area", element: id, area: created.id }))) return;
    areaTitle = "";
    chrome?.close();
  }
  async function selectionAction(action: "area" | "ask" | "remove", ids: string[]) {
    const owned = ownedElements(ids);
    if (action === "ask") {
      composerElement?.querySelector<HTMLElement>("textarea")?.focus();
      return;
    }
    if (action === "area") {
      const first = items.find((item) => item.id === owned[0])?.title.trim();
      await groupSelection(first ? clipText(first, 24).trimEnd() : m.work_env_area(), owned);
      return;
    }
    for (const id of owned) {
      if (!(await session.edit({ kind: "remove", element: id }))) return;
      if (inspected === id) inspected = null;
    }
  }
  function onPanelChange(panel: WorkEnvironmentPanel | null) {
    session.tabsIntroduced = true;
    if (panel === "notes") notesOpen = false;
  }
</script>

{#snippet tabPanel()}<WorkTabPicker
    {tabs}
    {spaceName}
    {currentTabId}
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
  <div class="palette" role="group" aria-label={m.work_env_components()}>
    {@render component(Table01Icon, m.work_env_component_table())}
    {@render component(ChartColumnIcon, m.work_env_component_chart())}
    {@render component(MapsIcon, m.work_env_component_map())}
    {@render component(LayoutGridIcon, m.work_env_area(), () => void createArea())}
  </div>
{/snippet}
{#snippet component(
  icon: typeof Table01Icon,
  label: string,
  onchoose: (() => void) | undefined = undefined,
)}
  <button type="button" class="menu-row" disabled={!onchoose || busy} onclick={onchoose}>
    <span class="menu-icon"><Icon {icon} /></span>{label}{#if !onchoose}<span class="soon"
        >{m.work_env_component_soon()}</span
      >{/if}
  </button>
{/snippet}
{#snippet areaPanel()}
  <div class="menu">
    <div class="menu-heading">{m.work_env_new_area_hint()}</div>
    <form
      class="menu-create"
      onsubmit={(event) => {
        event.preventDefault();
        if (!areaTitle.trim()) return;
        void createArea(areaTitle);
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
  <div class="palette-stack">
    <div class="segments" role="group" aria-label={m.work_env_media()}>
      {@render segment("document", File01Icon, m.work_env_documents())}
      {@render segment("image", Image01Icon, m.work_env_images())}
      {@render segment("link", Link04Icon, m.work_env_links())}
      {@render segment("folder", FolderAddIcon, m.work_env_folders())}
    </div>
    {#if mediaKind === "folder"}
      <form
        class="menu-create"
        onsubmit={(event) => {
          event.preventDefault();
          void addFolder(folderDraft.trim()).then((placed) => {
            if (placed) {
              folderDraft = "";
              chrome?.close();
            }
          });
        }}
      >
        <span class="menu-icon"><Icon icon={FolderAddIcon} /></span>
        <input
          aria-label={m.work_env_folder_placeholder()}
          placeholder={m.work_env_folder_placeholder()}
          bind:value={folderDraft}
          maxlength="1024"
          disabled={busy || folderPending}
        /><Button
          type="submit"
          size="compact"
          disabled={busy || folderPending || !folderDraft.trim()}>{m.work_env_folder_add()}</Button
        >
      </form>
      <div class="menu-footer">
        <Button size="compact" disabled={busy || folderPending} onclick={() => void chooseFolder()}
          >{m.work_env_folder_choose()}</Button
        >
      </div>
      <p class="menu-status">{m.work_env_folder_hint()}</p>
    {:else if mediaKind === "link"}
      <form
        class="menu-create"
        onsubmit={(event) => {
          event.preventDefault();
          void addLink();
        }}
      >
        <span class="menu-icon"><Icon icon={Link04Icon} /></span>
        <input
          type="url"
          aria-label={m.work_env_link_placeholder()}
          placeholder={m.work_env_link_placeholder()}
          bind:value={linkDraft}
          maxlength="2048"
          disabled={busy || linkPending}
        /><Button type="submit" size="compact" disabled={busy || linkPending || !linkDraft.trim()}
          >{m.work_env_link_add()}</Button
        >
      </form>
      {#if linkFailure}<p class="menu-status" role="alert">{m.work_env_link_failed()}</p>{/if}
    {:else}
      <WorkMediaPicker
        profile={session.profile}
        kind={mediaKind}
        attachedIds={snapshot?.elements.flatMap((element) =>
          element.reference.kind === "resource" ? [element.reference.resource] : [],
        ) ?? []}
        pending={busy}
        onattach={(ids) => void attachResources(ids)}
      />
    {/if}
  </div>
{/snippet}
{#snippet segment(
  key: "document" | "image" | "link" | "folder",
  icon: typeof File01Icon,
  label: string,
)}
  <button
    type="button"
    class="segment"
    aria-pressed={mediaKind === key}
    onclick={() => (mediaKind = key)}
  >
    <Icon {icon} size={14} />{label}
  </button>
{/snippet}
{#snippet notesPanel()}
  <button type="button" class="menu-row" disabled={busy} onclick={() => void createNote()}>
    <span class="menu-icon"><Icon icon={NoteAddIcon} /></span>{m.work_env_new_note()}
  </button>
  <div class="notes-panel">
    <LazyView
      loader={loadNotes}
      loadingLabel={m.surface_loading()}
      failureLabel={m.surface_render_failed()}
      retryLabel={m.surface_retry()}
      >{#snippet children(Notes)}{#if notes}<Notes session={notes} />{/if}{/snippet}</LazyView
    >
  </div>
  <div class="menu-footer">
    <Button
      size="compact"
      disabled={busy ||
        !notes?.note?.id ||
        snapshot?.elements.some(
          (element) =>
            element.reference.kind === "resource" && element.reference.resource === notes?.note?.id,
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
      >{#snippet children(Tasks)}<Tasks session={taskList} density="panel" />{/snippet}</LazyView
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
          onreview={(step: string) => {
            chrome?.close();
            inspected = null;
            liftFile = null;
            lifted = { id: "", origin: null, proposal: step };
          }}
          waiting={agentWaiting}
          onwaitingpage={(card: string) => {
            chrome?.close();
            canvasRef?.focusCard(card);
          }}
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
<svelte:window onbeforeunload={abandonTakeover} />
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
              clusters={clusters.clusters}
              areas={snapshot.areas}
              author={profileLabel}
              {pictures}
              initialView={canvasView}
              {remoteView}
              {authoritative}
              expose={(api) => (canvasRef = api)}
              fitBottomInset={composerHeight}
              still={!!lifted || !!pane || !!takeover}
              oninspect={(id: string) => (inspected = id)}
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
              onselectionaction={(action: "area" | "ask" | "remove", ids: string[]) =>
                void selectionAction(action, ids)}
              onareaedit={(
                area: string,
                edit: { kind: "rename"; title: string } | { kind: "remove" },
              ) =>
                void session.edit(
                  edit.kind === "rename"
                    ? { kind: "rename_area", area, title: edit.title }
                    : { kind: "remove_area", area },
                )}
              onevidence={(id: string, source: EvidenceReference) => void openResult(id, source)}
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
                if (action === "help") {
                  openTakeover(id);
                  return;
                }
                if (action === "play") {
                  playHere(id);
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
    {#if session.failure || session.pending || session.viewDraft || folderRefused}<div
        class="status"
        role="status"
      >
        <span
          >{session.failure
            ? m.work_request_failed()
            : session.pending
              ? m.work_env_pending()
              : session.viewDraft
                ? m.work_env_unsaved_view()
                : m.work_env_folder_refused()}</span
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
            const objective = results.remaining?.objective;
            const request = snapshot?.elements.find(
              (element) =>
                element.reference.kind === "objective" && element.reference.objective === objective,
            );
            if (request) lift(request.id);
          }}>{m.work_env_other_results({ count: results.remaining.count })}</Button
        >
      </div>{/if}
    {#if notesOpen}<section class="detail" aria-label={m.work_env_notes()}>
        <Button onclick={() => void closeNotes()}>{m.work_env_close()}</Button
        >{@render notesPanel()}
      </section>{/if}
  </div>
  {#if lifted && cardBounds}
    <Lift
      origin={lifted.origin}
      source={lifted.id || null}
      bounds={cardBounds}
      preferred={liftSize(liftedItem)}
      title={lifted.proposal ? m.work_line_review() : (liftedItem?.title ?? "")}
      onclose={() => {
        lifted = null;
        liftSource = null;
        liftFile = null;
      }}
    >
      {#if lifted.proposal && objectiveSession}
        <LazyView
          loader={loadFile}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(FileView)}<FileView
              proposal={{ session: objectiveSession!, step: lifted!.proposal! }}
              ondecided={() => (lifted = null)}
            />{/snippet}</LazyView
        >
      {:else if liftFile}
        <LazyView
          loader={loadFile}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(FileView)}<FileView
              file={liftFile!}
              onreveal={reveal}
              onback={() => (liftFile = null)}
            />{/snippet}</LazyView
        >
      {:else if liftedItem?.sources}
        <div class="lift-body">
          <LiftHeader
            kind={m.work_sources()}
            title={liftedItem.title}
            meta={m.work_env_sources_count({ count: liftedItem.sources.length })}
            icon={GlobalIcon}
          />
          <ul class="lift-sources">
            {#each liftedItem.sources as row (row.key)}
              <li>
                <button
                  type="button"
                  onclick={() => {
                    if (row.file) {
                      liftFile = fileEvidence(context.objectives, row.file.record) ?? null;
                      return;
                    }
                    const url = row.url;
                    lifted = null;
                    openPane({ kind: "url", url }, null);
                  }}
                >
                  <HostGlyph host={row.where} file={!!row.file} size={22} />
                  <span class="source-text">
                    <strong>{row.title}</strong>
                    <span>{row.note || row.where}</span>
                  </span>
                </button>
              </li>
            {/each}
          </ul>
          {#if foldedPages.length}{@const more =
              foldedPages.length === 1
                ? m.work_lift_more_pages_one()
                : m.work_lift_more_pages({ count: foldedPages.length })}
            <section class="lift-folded" aria-label={more}>
              <h3>{more}</h3>
              <ul class="lift-sources">
                {#each foldedPages as group (group.id)}<li>
                    <button
                      type="button"
                      onclick={() => {
                        lifted = null;
                        openPane({ kind: "url", url: group.url }, null);
                      }}
                    >
                      <HostGlyph host={host(group.url)} size={22} />
                      <span class="source-text">
                        <strong>{host(group.url) || group.url}</strong>
                        <span>{group.url}</span>
                      </span>
                    </button>
                  </li>{/each}
              </ul>
            </section>{/if}
        </div>
      {:else if liftedElement?.reference.kind === "resource" && liftedItem?.media}
        {@const asset = liftedItem.media.asset}
        {@const image =
          asset.kind === "image" ? mediaUrl(liftedItem.media.profile, asset.digest) : null}
        <div class="lift-body lift-media">
          <LiftHeader
            kind={liftedItem.kind}
            title={liftedItem.title}
            meta={mediaMeta(asset)}
            icon={asset.kind === "image"
              ? Image01Icon
              : asset.kind === "pdf"
                ? Pdf01Icon
                : File01Icon}
          >
            {#snippet actions()}{#if !image}<Button
                  size="compact"
                  onclick={() => {
                    const id =
                      liftedElement?.reference.kind === "resource"
                        ? liftedElement.reference.resource
                        : null;
                    if (id) void commands.mediaOpen(session.profile, id);
                  }}>{m.work_media_open_file()}</Button
                >{/if}{/snippet}
          </LiftHeader>
          {#if image}<div class="lift-picture"><img src={image} alt={liftedItem.title} /></div>{/if}
        </div>
      {:else if liftedElement?.reference.kind === "resource"}
        <LazyView
          loader={loadNoteHost}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(Host)}<Host
              profile={session.profile}
              host={`${notesHost}:lift`}
              id={liftedElement.reference.kind === "resource"
                ? liftedElement.reference.resource
                : ""}
            />{/snippet}</LazyView
        >
      {:else if liftedElement?.reference.kind === "folder"}
        {@const folder = liftedElement.reference.path}
        <div class="lift-body lift-plain">
          <LiftHeader
            kind={liftedItem?.kind ?? m.work_env_folder()}
            title={liftedItem?.title ?? ""}
            meta={homePath(folder)}
            icon={FolderAddIcon}
          >
            {#snippet actions()}<Button size="compact" onclick={() => reveal(folder)}
                >{m.work_env_reveal()}</Button
              >{/snippet}
          </LiftHeader>
          <p>{m.work_env_folder_grant_note()}</p>
        </div>
      {:else if liftedElement?.reference.kind === "subject"}
        {@const subject = liftedElement.reference}
        <LazyView
          loader={loadSubject}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(Product)}<Product
              reference={subject}
              objectives={context.objectives}
              pictures={liftedPictures}
              onopen={openCitation}
              onfile={(record: string) =>
                (liftFile = fileEvidence(context.objectives, record) ?? null)}
            />{/snippet}</LazyView
        >
      {:else if liftedElement?.reference.kind === "objective" && objectiveSession}
        <LazyView
          loader={loadDetail}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(Detail)}<Detail
              session={objectiveSession!}
              attached={snapshot?.elements.map((element) => element.reference) ?? []}
              onopencitation={openCitation}
              onattach={(reference) => void session.edit({ kind: "add", reference, area: null })}
            />{/snippet}</LazyView
        >
      {:else if liftedItem?.artifact}
        <div class="lift-result">
          {#if results.references.get(liftedItem.id) && objectiveSession}
            <LazyView
              loader={loadResult}
              loadingLabel={m.surface_loading()}
              failureLabel={m.surface_render_failed()}
              retryLabel={m.surface_retry()}
              >{#snippet children(Result)}<Result
                  session={objectiveSession!}
                  reference={results.references.get(liftedItem!.id)!}
                  source={liftSource}
                  {pictures}
                  onopen={openCitation}
                  onfile={(record: string) =>
                    (liftFile = fileEvidence(context.objectives, record) ?? null)}
                />{/snippet}</LazyView
            >
          {:else}<LiftHeader kind={liftedItem.kind} title={liftedItem.title} />{/if}
        </div>
      {:else if liftedItem && liftedCommand}
        <div class="lift-body">
          <LiftHeader
            kind={liftedItem.kind}
            title={liftedItem.title}
            meta={liftedItem.command?.reason ?? ""}
            icon={ComputerTerminal01Icon}
          />
          {#await import("./local/CommandRecord.svelte") then module}
            <module.default record={liftedCommand} />
          {/await}
        </div>
      {:else if liftedItem}
        <div class="lift-body lift-plain">
          <LiftHeader
            kind={liftedItem.kind}
            title={liftedItem.title}
            meta={[liftedItem.detail, liftedItem.status].filter(Boolean).join(" · ")}
            icon={liftedElement?.reference.kind === "browser" ? GlobalIcon : undefined}
            primary={liftedElement?.reference.kind === "browser"
              ? { label: m.work_env_open_page(), onclick: openLiftedTab }
              : undefined}
          />
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
  {#if takeoverView}
    <LazyView
      loader={loadTakeover}
      loadingLabel={m.surface_loading()}
      failureLabel={m.surface_render_failed()}
      retryLabel={m.surface_retry()}
      >{#snippet children(Takeover)}<Takeover
          host={takeoverView.current.host}
          url={takeoverView.current.url}
          bounds={takeoverView.bounds}
          page={takeoverView.page}
          error={takeoverError}
          onregion={takeoverRegion}
          oncontinue={continueTakeover}
          onclose={() => endTakeover(true)}
        />{/snippet}</LazyView
    >
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

  .lift-body {
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-block-size: 0;
  }

  .lift-folded h3 {
    margin: 0 0 4px;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 500;
  }

  .lift-sources {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
  }

  .lift-sources button {
    display: flex;
    align-items: center;
    gap: 12px;
    inline-size: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-instant) ease;
  }

  .lift-sources button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .lift-sources button:hover {
    background: var(--color-fill-hover);
  }

  .source-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .source-text strong {
    font-weight: 550;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .source-text span {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .lift-media {
    block-size: 100%;
  }

  .lift-picture {
    display: grid;
    flex: 1;
    place-items: center;
    min-block-size: 0;
  }

  .lift-picture img {
    max-inline-size: 100%;
    max-block-size: 100%;
    object-fit: contain;
    border-radius: var(--radius-row);
  }

  .lift-plain p {
    margin: 0;
    color: var(--color-muted);
  }

  .detail {
    position: absolute;
    inset-block: 16px;
    inset-inline-end: 16px;
    inline-size: min(520px, calc(100% - 32px));
    box-sizing: border-box;
    overflow: auto;
    padding: 16px;
    border-radius: var(--radius-menu);
    background: var(--color-menu);
    backdrop-filter: blur(14px) saturate(1.2);
    box-shadow: var(--shadow-popover);
    z-index: 20;
  }

  .menu {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .palette {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 2px;
  }

  .palette-stack {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-inline-size: 0;
  }

  .soon {
    margin-inline-start: auto;
    padding: 1px 7px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-active);
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .segments {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
  }

  .segment {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    flex: 1;
    min-inline-size: 0;
    block-size: 28px;
    overflow: hidden;
    white-space: nowrap;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    cursor: default;
  }

  .segment:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .segment[aria-pressed="true"] {
    background: var(--color-surface);
    color: var(--color-text);
    box-shadow: var(--shadow-control);
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
