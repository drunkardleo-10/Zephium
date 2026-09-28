<script lang="ts">
  import { untrack, onMount, setContext } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import { WorkEnvironmentContext, type WorkEnvironmentSession } from "$domain/work-environment";
  import { commandId, workSession, type WorkSession } from "$domain/work";
  import { taskSession } from "$domain/resources";
  import { noteSession } from "$domain/notes";
  import * as toolHost from "$session/tools.svelte";
  import { tabRequest } from "$session/work-tab.svelte";
  import { pointerTool, setPointerTool } from "../lib/pointer-tool.svelte";
  import { ulidTime } from "../lib/ulid-time";
  import { editTool, toolSession } from "$session/tool-drafts.svelte";
  import type {
    TabView,
    WorkHumanAccountV1,
    WorkHumanPageIdV1,
    WorkHumanRegionV1,
    WorkEnvironmentReference,
    WorkFileEvidenceV1,
    WorkAccountEffectV1,
    WorkAccountModeV1,
    WorkContextSelectionV1,
    WorkEnvironmentSnapshot,
    WorkExecutionFact,
    WorkRuntimeProjection,
  } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import { layout } from "$domain/layout";
  import { workPane, type WorkPaneRect, type WorkPaneTarget } from "$domain/work-pane";
  import { WorkHumanSession, type WorkHumanFailure } from "$domain/work-human";
  import { loadNoteHost } from "$features/notes";
  import Button from "$shared/ui/Button";
  import LazyView from "$shared/ui/LazyView";
  import {
    ComputerTerminal01Icon,
    File01Icon,
    FolderAddIcon,
    GlobalIcon,
    Image01Icon,
    Pdf01Icon,
    PlusSignIcon,
    StickyNote03Icon,
    Cursor01Icon,
    HandIcon,
  } from "../lib/icons";
  import WorkBar from "./bar/WorkBar.svelte";
  import BarTool from "./bar/BarTool.svelte";
  import NotePanel from "./bar/NotePanel.svelte";
  import AttachPanel, { type AttachKind } from "./bar/AttachPanel.svelte";
  import AccountButton from "./top/AccountButton.svelte";
  import WorksMenu from "./top/WorksMenu.svelte";
  import ContextManifest from "./composer/ContextManifest.svelte";
  import AccountScopeChip from "./composer/AccountScopeChip.svelte";
  import OpenTabsChip from "./composer/OpenTabsChip.svelte";
  import AccountGrantReview from "./AccountGrantReview.svelte";
  import { contextSelection } from "../lib/context-selection";
  import WorkTabPicker from "./WorkTabPicker.svelte";
  import WorkMediaPicker from "./WorkMediaPicker.svelte";
  import { mediaSize, mediaUrl } from "$domain/resources";
  import BrowserPane from "./pane/BrowserPane.svelte";
  import Lift from "./Lift.svelte";
  import LiftHeader from "./LiftHeader.svelte";
  import { commandRecord } from "../lib/project-environment-stage";
  import {
    clearOfBands,
    elementPictures,
    environmentStages,
    laneElement,
    measureKey,
  } from "../lib/project-environment-board";
  import type { BoardActions } from "../lib/canvas-context";
  import HostGlyph from "./cards/HostGlyph.svelte";
  import { clipText, defaultSize } from "../lib/canvas-model";
  import { homePath } from "../lib/work-files";
  import { youtubeThumbnail } from "../lib/link-media";
  import { resultPlan } from "../lib/plan-steps";
  import { stepPlan, WorkTasks, workTasksKey, type StepPlan } from "../lib/work-tasks";
  import { documentMarkdown, resultKey, WorkNotes } from "../lib/work-notes";
  import type { LiftAction } from "./LiftHeader.svelte";
  import { isAgentExecution, isLive } from "../lib/agent-steps";
  import { environmentPlan } from "../lib/project-environment-plan";
  import {
    environmentAgents,
    environmentBoards,
    environmentItems,
    environmentLinks,
    environmentInputs,
    environmentParts,
    environmentPictures,
    environmentView,
    fileEvidence,
    viewPlacements,
  } from "../lib/project-environment";
  import { canvasProbe } from "../lib/canvas-context";
  import { environmentRequests } from "../lib/project-environment-thread";
  import { artifactView } from "../lib/project-work";
  import RunView from "./board/RunView.svelte";
  import { failureLine, humanPage, regionOf, sameRegion } from "../lib/work-human";
  import { openOver, type PaneRect } from "../lib/pane-geometry";
  import { organizeExecution, pendingOrganize, elementFor } from "../lib/organize";
  import { subjectImageCandidates, subjectsOf } from "../lib/subjects";
  import { environmentResults, type ResultReference } from "../lib/project-environment-results";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import type { CanvasView, CanvasItem, CanvasPosition } from "../lib/canvas-model";
  import { currentModel } from "../lib/model-name";
  import * as m from "$shared/i18n/messages";
  let {
    session,
    tabs,
    spaceName,
    profileLabel,
    currentTabId = null,
    aiEnabled = true,
    onopen,
    onnewtab,
  }: {
    session: WorkEnvironmentSession;
    tabs: readonly TabView[];
    spaceName: string;
    profileLabel: string;
    /** The Space's current tab, so the picker reads like its tab strip. */
    currentTabId?: string | null;
    aiEnabled?: boolean;
    /** Explicit Browse handoff for one Space tab; the pane is the default way to look at a page. */
    onopen: (id: string) => void;
    onnewtab: () => void;
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
  const notesHost = $derived(`environment:${session.space}`);
  const notes = untrack(() => noteSession(session.profile, notesHost));
  onMount(() => {
    void notes?.start();
    return () => notes?.stop();
  });
  // Steps read their tasks from a session of their own, whose scope never changes.
  const stepTaskSession = untrack(() => taskSession(session.profile, `work:${session.space}`));
  onMount(() => {
    let live = true;
    void stepTaskSession.start().then(async () => {
      // A lane's tasks may sit past the first page; a few pages cover a working list.
      while (live && stepTaskSession.next && stepTaskSession.items.length < 500)
        await stepTaskSession.reload(true);
    });
    return () => {
      live = false;
      stepTaskSession.stop();
    };
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
    reveal: (id: string) => void;
    followAgent: () => boolean;
  }>();
  let selectedIds = $state.raw<string[]>([]);
  let accountEffect = $state.raw<WorkAccountEffectV1>({ kind: "read" });
  /** The person's consent to list their open tabs, for the next request only. */
  let openTabs = $state(false);
  const contextSel = $derived(
    contextSelection(
      session.snapshot,
      selectedIds,
      tabs,
      {
        notes: context.notes,
        objectives: context.objectives,
        media: context.mediaRevisions,
      },
      openTabs,
    ),
  );
  let cardHost = $state<HTMLElement>();
  let cardBounds = $state.raw<DOMRect | null>(null);
  $effect(() => {
    const element = cardHost;
    if (!element) return;
    const measure = () => (cardBounds = element.getBoundingClientRect());
    measure();
    // Size alone misses a move: the column beside the canvas can change width
    // while the canvas keeps its own, and a pane placed on stale bounds lands
    // off the window.
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    window.addEventListener("resize", measure);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
    };
  });
  function openLift(id: string) {
    const item = items.find((item) => item.id === id);
    if (!item) return;
    // A block opens where it stands; its board makes room.
    if (item.type === "block") {
      // A reviewed plan's result keeps its review: it opens in the lift, on its run.
      if (results.references.has(id) && !agentBlock(id)) void openResult(id);
      else toggleBlock(id);
      return;
    }
    if (item.type === "head" || item.type === "trail") return;
    if (item.type === "objective" || item.type === "request") {
      lift(id);
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
      else openPane({ kind: "url", url: item.page.url }, id, item.page.account);
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
  /** A page in a branch: the person's turn opens the run's own view; any other opens as a page. */
  function openBranchPage(page: string) {
    const entry = pageEntries.get(page);
    if (!entry) return;
    if (entry.page.human?.phase === "waiting_for_human") {
      openTakeover(page);
      return;
    }
    openPane({ kind: "url", url: entry.page.url }, null, entry.page.account);
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
    panel = null;
    inspected = null;
    liftRecord = null;
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
    /** The page was read with the person's session on this host. */
    account?: string;
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
  // A tab chosen in the rail opens here, over the canvas.
  let tabAnswered = tabRequest()?.sequence ?? 0;
  $effect(() => {
    const request = tabRequest();
    if (!request || request.sequence === tabAnswered) return;
    tabAnswered = request.sequence;
    untrack(() => {
      if (tabs.some((tab) => tab.id === request.tab))
        openPane({ kind: "tab", id: request.tab }, null);
    });
  });
  function openPane(target: WorkPaneTarget, originId: string | null, account?: string) {
    if (cardHost) cardBounds = cardHost.getBoundingClientRect();
    panel = null;
    lifted = null;
    endTakeover(true);
    const restore =
      pane?.restore ??
      (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    const origin = originId ? (canvasRef?.screenRect(originId) ?? null) : null;
    const rect = paneRect;
    pane = {
      target,
      origin: pane ? pane.origin : origin,
      phase: "opening",
      restore,
      ...(account ? { account } : {}),
    };
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
  function openTakeover(page: string) {
    const entry = pageEntries.get(page);
    const item = entry ? undefined : items.find((candidate) => candidate.id === page);
    const state = entry?.page.human ?? item?.page?.human;
    // The well stands over the part the page is in.
    const card = entry?.part ?? page;
    const url = entry?.page.url ?? item?.page?.url ?? "";
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
    panel = null;
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
      host: host(url),
      url,
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
    if (item.artifact?.content.kind === "table" || item.artifact?.content.kind === "comparison")
      return { width: 960, height: 640 };
    switch (item.type) {
      case "note":
        return { width: 760, height: 620 };
      case "tab":
        return { width: 520, height: 260 };
      case "objective":
      case "request":
        return { width: 640, height: 600 };
      case "responsibility":
        return { width: 480, height: 320 };
      case "sources":
      case "part":
        return { width: 560, height: 560 };
      case "subject":
        // The picture and its facts at 640, the sources at 320.
        return { width: 1040, height: 720 };
      default: {
        const base = defaultSize(item);
        return { width: Math.max(720, base.width + 200), height: Math.max(520, base.height + 160) };
      }
    }
  }
  /** The bar's one open panel: a tool's, or the field's own attach. */
  let panel = $state<"note" | "tabs" | "media" | "attach" | null>(null);
  let attachKind = $state<AttachKind>("tabs");
  let objectivePending = $state(false);
  let composerFailure = $state<"account" | null>(null);
  let composerElement = $state<HTMLElement>();
  let composerHeight = $state(0);
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
      if (await placeFolder(chosen.data, null)) panel = null;
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
    type: CanvasItem["type"],
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
  async function addLink(raw: string) {
    const url = linkUrl(raw);
    if (!url || busy) return false;
    if (!(await place({ kind: "link", url, title: host(url) }, "link", null))) return false;
    panel = null;
    return true;
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
  /** The block opened in place, and every block's height as it measured itself. */
  let openBlock = $state<string | null>(null);
  const measured = new SvelteMap<string, number>();
  /** Blocks the person dragged: only those can leave their board's flow. */
  const moved = new SvelteSet<string>();
  function toggleBlock(id: string) {
    inspected = null;
    openBlock = openBlock === id ? null : id;
  }
  // Escape closes the block that is open, when nothing sits over the canvas.
  $effect(() => {
    if (!openBlock) return;
    const close = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || lifted || pane || takeover) return;
      openBlock = null;
    };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  });
  const chosen = $derived(
    new Set(
      (snapshot?.decisions ?? []).flatMap((decision) =>
        decision.choice ? [decision.element] : [],
      ),
    ),
  );
  /** Requests the person opened to read whole. */
  const openRequests = new SvelteSet<string>();
  /** Every request of the canvas as a band: its words, its branches, its result. */
  const stages = $derived(
    snapshot
      ? environmentStages(snapshot, context.objectives, {
          recorded: recordedPages,
          pictures: elementPictures(snapshot, context.media),
          chosen,
          measured,
          open: openBlock,
          requests: openRequests,
        })
      : [],
  );
  const clock = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
  /** When each band's request was asked: its first run carries the time in its id. */
  const askedAt = $derived(
    new Map(
      stages.flatMap((stage) => {
        const first = stage.executions[0];
        const at = first ? ulidTime(first) : null;
        return at ? [[stage.card, clock.format(at)] as const] : [];
      }),
    ),
  );
  // A run that ends while the person follows it hands them its result, at reading size.
  let wasLive = new Set<string>();
  $effect(() => {
    const now = new Set(stages.filter((stage) => stage.live).map((stage) => stage.card));
    untrack(() => {
      for (const card of wasLive) {
        if (now.has(card)) continue;
        const stage = stages.find((entry) => entry.card === card);
        if (stage && canvasRef?.followAgent()) canvasRef.reveal(stage.column.head ?? card);
      }
      wasLive = now;
    });
  });
  // A new work names itself after its first result, once, unless the person already has.
  const named: Record<string, true> = {};
  $effect(() => {
    const current = snapshot;
    const title = stages.find((stage) => stage.board.title.trim())?.board.title.trim();
    if (!current || !title || current.title !== m.work_env_default_title()) return;
    if (current.lifecycle !== "active" || named[current.id]) return;
    named[current.id] = true;
    untrack(() => void session.edit({ kind: "rename", title: title.slice(0, 64) }));
  });
  const agents = $derived(
    snapshot
      ? environmentAgents(snapshot, context.objectives, signalOf, stages)
      : { items: [], links: [], positions: {} },
  );
  const parts = $derived(
    environmentParts(
      context.objectives,
      stages,
      recordedPages,
      signalOf,
      (objective) => human.pages.get(objective) ?? [],
    ),
  );
  /** Every page a run shows, by id, with the part it stands in. */
  const pageEntries = $derived(
    new Map(
      parts.flatMap((item) =>
        (item.part?.pages ?? []).map((page) => [page.id, { part: item.id, page }] as const),
      ),
    ),
  );
  const boards = $derived(environmentBoards(stages, openBlock, blockArtifact));
  /** The page this canvas's run is held on, so the line can point at its card. */
  const agentWaiting = $derived.by(() => {
    const work = objectiveSession?.projection?.work.id;
    const opened = work ? (human.pages.get(work) ?? []) : [];
    if (!opened.length) return null;
    for (const { page } of pageEntries.values()) {
      const state = page.human;
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
        card: page.id,
        host: host(page.url),
        reason: state.reason,
        remaining: state.remaining,
      };
    }
    return null;
  });
  const requests = $derived(environmentRequests(stages));
  const items = $derived([
    ...[...results.items, ...requests.items].map((item) =>
      item.type === "objective" || item.type === "request"
        ? {
            ...item,
            expanded: openRequests.has(item.id),
            ...(askedAt.get(item.id) ? { when: askedAt.get(item.id) } : {}),
          }
        : item,
    ),
    ...boards,
    ...parts,
    ...environmentInputs(stages),
    ...agents.items,
  ]);
  const links = $derived([
    ...scene.links,
    ...(snapshot ? environmentLinks(snapshot) : []),
    ...requests.links,
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
      if (saved.has(element.id) || laneElement(snapshot, element)) continue;
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
      // What a run places lands on its request's board; the anchor is only a record.
      const anchor = place ? { x: place.x, y: place.y } : { x: 80, y: 120 };
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
  /** Every result with steps, as tasks would carry them. */
  const stepPlans = $derived(
    new Map(
      items.flatMap((item): [string, StepPlan][] => {
        const reference = results.references.get(item.id);
        const steps = item.artifact ? resultPlan(item.artifact.content) : [];
        return reference && item.artifact && steps.length
          ? [[item.id, stepPlan(item.id, reference.objective, item.artifact, steps)]]
          : [];
      }),
    ),
  );
  const workTasks = untrack(
    () => new WorkTasks(session.profile, stepTaskSession, () => stepPlans, openTask),
  );
  setContext(workTasksKey, workTasks);
  // The lift asks for a site's icon as the canvas does: once per origin.
  const probedOrigins: Record<string, true> = {};
  setContext(canvasProbe, (origin: string) => {
    if (probedOrigins[origin]) return;
    probedOrigins[origin] = true;
    void commands.faviconProbe(session.profile, [origin]).catch(() => false);
  });
  /** A step's task opens in the sidebar's Tasks, listed whatever its day. */
  function openTask(id: string) {
    panel = null;
    lifted = null;
    editTool(toolSession("sidebar", session.profile, "tasks"), { filter: "all" });
    const tasks = taskSession(session.profile, "sidebar");
    tasks.selectedId = id;
    toolHost.open("tasks");
    void tasks.load(id);
  }
  const workNotes = new WorkNotes();
  /** A document result written as the person's note, only when they ask. */
  async function saveNote(id: string) {
    const reference = results.references.get(id);
    const view = items.find((item) => item.id === id)?.artifact;
    const markdown = view ? documentMarkdown(view) : null;
    if (!reference || !markdown) return;
    await workNotes.save(resultKey(reference), markdown, notes);
  }
  /** A kept note opens in the sidebar's Notes, beside the canvas. */
  function openNote(id: string) {
    panel = null;
    lifted = null;
    toolHost.open("notes");
    void noteSession(session.profile, "sidebar")?.requestOpen(id);
  }
  /** Save as note until the note exists, then Open note. */
  function noteAction(id: string): LiftAction | undefined {
    const reference = results.references.get(id);
    // A document or an answer: whatever has Markdown to keep.
    const view = items.find((item) => item.id === id)?.artifact;
    if (!reference || !view || !documentMarkdown(view)) return undefined;
    const key = resultKey(reference);
    const note = workNotes.note(key);
    if (note) return { label: m.work_open_note(), onclick: () => openNote(note) };
    const saving = workNotes.saving(key);
    return {
      label: saving ? m.work_saving_note() : m.work_save_note(),
      disabled: saving,
      onclick: () => void saveNote(id),
    };
  }
  /** A subject written up as a note, once; then its note opens. */
  function subjectNote(
    subject: Extract<WorkEnvironmentReference, { kind: "subject" }>,
    markdown: string,
  ): LiftAction {
    const key = `subject:${subject.objective}:${subject.execution}:${subject.artifact}:${subject.index}`;
    const note = workNotes.note(key);
    if (note) return { label: m.work_open_note(), onclick: () => openNote(note) };
    const saving = workNotes.saving(key);
    return {
      label: saving ? m.work_saving_note() : m.work_save_note(),
      disabled: saving,
      onclick: () => void workNotes.save(key, markdown, notes),
    };
  }
  let liftRef = $state<Lift>();
  /** Ask about this: the composer takes the subject's name, and the lift steps back. */
  function askAbout(name: string) {
    session.composer = m.work_lift_ask_prefix({ name });
    liftRef?.close();
    requestAnimationFrame(() => {
      const field = composerElement?.querySelector<HTMLTextAreaElement>("textarea");
      field?.focus();
      field?.setSelectionRange(field.value.length, field.value.length);
    });
  }
  /** The document the agent line's run just wrote, if it wrote one. */
  const lineDocument = $derived.by(() => {
    const projection = objectiveSession?.projection;
    const execution = projection?.executions.at(-1);
    if (!projection || !execution) return null;
    for (const [id, reference] of results.references)
      if (
        reference.objective === projection.work.id &&
        reference.execution === execution.id &&
        items.find((item) => item.id === id)?.artifact?.content.kind === "document"
      )
        return { id, key: resultKey(reference) };
    return null;
  });
  const writeup = $derived(
    lineDocument && !workNotes.note(lineDocument.key) && !workNotes.saving(lineDocument.key)
      ? () => void saveNote(lineDocument.id)
      : undefined,
  );
  /** The lift's one action for a result: Make tasks for a plan, Save as note for a document. */
  function resultAction(id: string): LiftAction | undefined {
    const state = workTasks.state(id);
    if (state === "none") return noteAction(id);
    return {
      label:
        state === "made"
          ? m.work_tasks_made()
          : state === "making"
            ? m.work_making_tasks()
            : m.work_make_tasks(),
      title: m.work_make_tasks_hint(),
      disabled: state !== "ready",
      onclick: () => void workTasks.make(id),
    };
  }
  /** A trail's command opened whole, as the run recorded it. */
  let liftRecord = $state<string | null>(null);
  const liftedCommand = $derived(
    liftRecord ? commandRecord(context.objectives, liftRecord) : undefined,
  );
  /** Whether a block came from an agent's run rather than a reviewed plan. */
  function agentBlock(id: string) {
    const reference = results.references.get(id);
    const execution = reference
      ? context.objectives
          .get(reference.objective)
          ?.executions.find((entry) => entry.id === reference.execution)
      : undefined;
    return !!execution && isAgentExecution(execution);
  }
  /** What a block is when it is one published result: its note and its tasks come from it. */
  function blockArtifact(id: string) {
    const reference = snapshot?.elements.find((element) => element.id === id)?.reference;
    if (reference?.kind !== "artifact") return undefined;
    const execution = context.objectives
      .get(reference.objective)
      ?.executions.find((entry) => entry.id === reference.execution);
    const artifact = execution?.artifacts.find((entry) => entry.id === reference.artifact);
    return execution && artifact ? artifactView(artifact, execution) : undefined;
  }
  const boardActions: BoardActions = {
    measure(id, width, open, height) {
      const key = measureKey(id, width, open);
      if (measured.get(key) !== height) measured.set(key, height);
    },
    toggle: toggleBlock,
    ask: (name) => askAbout(name),
    choose(element, choose) {
      void session.edit(
        choose
          ? { kind: "decide", element, choice: m.work_env_chosen() }
          : { kind: "undecide", element },
      );
    },
    evidence(reference) {
      if (reference.file) {
        liftFile = fileEvidence(context.objectives, reference.file.record) ?? null;
        if (liftFile) lifted = { id: "", origin: null };
        return;
      }
      if (reference.url) openCitation(reference.url);
    },
    entity: (element) => lift(element),
    command(record) {
      panel = null;
      liftFile = null;
      liftRecord = record;
      lifted = { id: "", origin: null };
    },
    page: (url) => openCitation(url),
    note: (id) => noteAction(id),
  };
  const liftedElement = $derived(snapshot?.elements.find((element) => element.id === lifted?.id));
  /** A request opens its run: what it did, its steps, its sources. */
  const liftedStage = $derived(stages.find((stage) => stage.card === lifted?.id));
  /** A reviewed plan's objective keeps its plan view; an agent's request opens its run. */
  const liftedAgent = $derived.by(() => {
    const stage = liftedStage;
    if (!stage) return false;
    // The run the canvas is watching is the freshest word on what this request is.
    const projection =
      objectiveSession?.projection?.work.id === stage.objective
        ? objectiveSession.projection
        : context.objectives.get(stage.objective);
    return !!projection?.executions.some(
      (run) => stage.executions.includes(run.id) && isAgentExecution(run),
    );
  });

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
  const loadDetail = () => import("./WorkObjectiveInspector.svelte");
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
            ...plannedGeometry.positions,
            ...planGeometry.positions,
            ...savedResultPositions,
            ...clearOfBands(
              environmentView(snapshot).positions,
              environmentView(snapshot).sizes ?? {},
              stages,
            ),
            ...requests.positions,
            ...agents.positions,
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
    panel = null;
  }
  /** A new note joins the canvas and opens over it, ready to write in. */
  async function createNote() {
    const current = notes;
    const environment = session.snapshot?.id;
    if (!current || busy) return;
    await current.start();
    // A note has no file, and so no identity to attach, until it has text.
    await current.create(`# ${m.note_untitled()}\n`);
    const id = current.note?.id;
    if (!id || !(await current.flush()) || environment !== session.snapshot?.id) return;
    const reference = { kind: "resource" as const, resource: id };
    if (!(await place(reference, "link", null))) return;
    const element = session.snapshot ? elementFor(session.snapshot, reference) : undefined;
    if (element) lift(element.id);
  }
  /** The bar's keys: V and H pick the pointer, N and A open their panels, / goes to the ask. */
  function toolKeys(event: KeyboardEvent) {
    if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return;
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (target?.closest("input, textarea, select, [contenteditable], [role='dialog']")) return;
    if (!snapshot || pane || lifted || takeover) return;
    const key = event.key.toLowerCase();
    if (key === "v" || key === "h") setPointerTool(key === "v" ? "select" : "hand");
    else if (key === "n") openPanel("note", true);
    else if (key === "a") openPanel("media", true);
    else if (key === "/" && aiEnabled)
      composerElement?.querySelector<HTMLElement>("textarea")?.focus();
    else return;
    event.preventDefault();
  }
  /** One of the person's own notes, placed on the canvas as it is. */
  async function placeNote(id: string) {
    const reference = { kind: "resource" as const, resource: id };
    if (await place(reference, "link", null)) panel = null;
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
  /**
   * A read that met a sign-in wall: the run stops there, the page opens in the
   * pane for the person to sign in, and the same request can then be sent as
   * them, once they allow the grant Rust drafts.
   */
  let signInRetry = $state.raw<{ work: string; origin: string; host: string } | null>(null);
  async function signIn(card: string) {
    const current = objectiveSession;
    const url = pageEntries.get(card)?.page.url;
    const work = current?.projection?.work.id;
    const execution = current?.projection?.executions.at(-1);
    if (!current || !url || !work) return;
    let origin: string;
    try {
      origin = new URL(url).origin;
    } catch {
      return;
    }
    if (execution && current.projection && isLive(current.projection, execution)) {
      const stopped = await current.execute({
        kind: "cancel",
        execution: execution.id,
        intervention: { kind: "sign_in", origin },
      });
      if (!stopped) return;
    }
    signInRetry = { work, origin, host: new URL(origin).host };
    openPane({ kind: "url", url }, card);
  }
  async function retrySignedIn() {
    const current = objectiveSession;
    const retry = signInRetry;
    if (!current || !retry || current.projection?.work.id !== retry.work) return;
    closePane();
    signInRetry = null;
    await current.retrySignedIn(retry.origin);
  }
  /** A field change needs both values, and different ones, before it is sent. */
  function accountInvalid() {
    const account = session.accountScope;
    return (
      !!account &&
      account.mode === "page" &&
      accountEffect.kind === "update" &&
      (!accountEffect.update.from.trim() ||
        !accountEffect.update.to.trim() ||
        accountEffect.update.from === accountEffect.update.to)
    );
  }
  function clearAccount() {
    session.accountScope = null;
    accountEffect = { kind: "read" };
  }
  /** One field, one meaning: the first message starts the work, the rest continue it. */
  async function send() {
    const text = session.composer.trim();
    if (!text || busy) return;
    const current = objectiveSession;
    if (runningObjective && current) {
      const account = session.accountScope;
      if (account && !activeExecution) {
        await continueSignedIn(current, text, account);
        return;
      }
      const context = contextSel;
      session.composer = "";
      openTabs = false;
      // Words typed while the agent works steer it now; if it cannot take
      // them mid-step, they go next.
      if (activeExecution) {
        if (!(await current.steer(text))) current.enqueue(text);
      } else await current.continueWith(text, context);
      return;
    }
    await createObjective();
  }
  /** The next message of a work, read with the tab's signed-in session. */
  async function continueSignedIn(
    current: WorkSession,
    text: string,
    account: NonNullable<typeof session.accountScope>,
  ) {
    const environment = session.snapshot?.id;
    if (!environment) return;
    if (accountInvalid()) {
      composerFailure = "account";
      return;
    }
    composerFailure = null;
    const context = account.mode === "origin" ? contextSel : null;
    const effect = accountEffect;
    if (!(await current.edit({ kind: "set_objective", objective: text }))) return;
    const work = current.projection?.work;
    if (!work) return;
    session.composer = "";
    openTabs = false;
    clearAccount();
    await prepareSignedIn(current, work.id, work.revision, environment, account, effect, context);
  }
  async function prepareSignedIn(
    current: WorkSession,
    work: string,
    revision: string,
    environment: string,
    account: { element: string; mode?: WorkAccountModeV1 },
    effect: WorkAccountEffectV1,
    context: WorkContextSelectionV1 | null,
  ) {
    const request = {
      version: 1,
      work,
      expected_revision: revision,
      environment,
      element: account.element,
    };
    if (account.mode === "origin") await current.prepareGrant(request, context);
    else
      await current.operations.begin({
        kind: "prepare_account",
        request: { ...request, effect },
      });
  }
  async function createObjective() {
    if (!session.composer.trim() || objectivePending || busy) return;
    const current = workSession(session.profile);
    if (!current) return;
    const account = session.accountScope;
    if (accountInvalid()) {
      composerFailure = "account";
      return;
    }
    const submission = session.objectiveSubmission ?? {
      objective: session.composer.trim(),
      command: commandId(),
      attached: false,
      // An origin grant serves an ordinary request: its context rides along once allowed.
      context: account?.mode === "page" ? null : contextSel,
      account: account
        ? { element: account.element, effect: accountEffect, mode: account.mode }
        : null,
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
      openTabs = false;
      clearAccount();
      const environmentId = session.snapshot?.id;
      if (submission.account && environmentId)
        await prepareSignedIn(
          current,
          objectiveId,
          basis.revision,
          environmentId,
          submission.account,
          submission.account.effect,
          submission.context,
        );
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
      placements: viewPlacements(snapshot, view, stages, moved),
    };
    if (
      JSON.stringify({ ...next, revision: "" }) !==
      JSON.stringify({ ...(session.viewDraft?.view ?? snapshot.view), revision: "" })
    )
      session.checkpoint(next);
  }
  const activeExecution = $derived.by(() => {
    const projection = objectiveSession?.projection;
    return !!projection?.executions.some((execution) => isLive(projection, execution));
  });
  // The sidebar marks the project whose run is live; a run keeps its project when the canvas moves on.
  $effect(() => {
    const live = activeExecution;
    const here = runningObjective ? snapshot?.id : undefined;
    untrack(() => {
      if (!live) session.running = null;
      else if (here) session.running = here;
    });
  });
  const needsDecision = $derived(
    !!objectiveSession?.projection?.work.questions.some((question) => question.state === "active"),
  );
  /** One click makes an Area: around the selection when there is one. */
  const ownedElements = (ids: readonly string[]) =>
    ids.filter((id) => snapshot?.elements.some((element) => element.id === id));
  /** A new area around the person's selected elements; the canvas places it before they move in. */
  async function groupSelection(title: string, ids: readonly string[] = selectedIds) {
    const bounds = canvasRef?.selectionBounds(ownedElements(ids));
    const current = snapshot;
    if (!bounds || !current || !title.trim()) return false;
    if (!(await session.flushView())) return false;
    if (!(await session.edit({ kind: "create_area", title: title.trim() }))) return false;
    const created = session.snapshot?.areas.find(
      (area) => !current.areas.some((known) => known.id === area.id),
    );
    if (!created) return false;
    const { ids: members, ...rect } = bounds;
    canvasRef?.placeArea(created.id, rect);
    for (const id of members)
      if (!(await session.edit({ kind: "assign_area", element: id, area: created.id })))
        return false;
    panel = null;
    return true;
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
  /** Opens one of the bar's panels; the tab and media tools open the attach panel on their kind. */
  function openPanel(next: typeof panel, open: boolean) {
    if (!open) {
      if (panel === next) panel = null;
      return;
    }
    if (next === "tabs") attachKind = "tabs";
    panel = next;
  }
  const model = $derived(currentModel(objectiveSession?.projection));
  const grantOpen = $derived(
    runningObjective && !!(objectiveSession?.grantDraft || objectiveSession?.grantDeclined),
  );
  /** The run on this canvas is going, or waits on the person: the bar is its line. */
  const lineRunning = $derived(
    runningObjective && (activeExecution || needsDecision || !!agentWaiting),
  );
  // Saving is silent; only a write that did not land says so.
  const troubled = $derived(
    !!session.failure ||
      folderRefused ||
      session.delivery === "unknown" ||
      session.delivery === "conflict" ||
      session.delivery === "rejected",
  );
  const runStatus = $derived(
    runningObjective && needsDecision
      ? m.work_env_needs_you()
      : runningObjective && activeExecution
        ? m.work_env_working()
        : null,
  );
  // A different project on the canvas starts with nothing lifted or open over it.
  let shownProject: string | undefined;
  $effect(() => {
    const id = snapshot?.id;
    untrack(() => {
      if (shownProject !== undefined && id !== shownProject) {
        inspected = null;
        lifted = null;
        panel = null;
      }
      shownProject = id;
    });
  });
</script>

{#snippet attachPanel()}<AttachPanel
    bind:kind={attachKind}
    {busy}
    {folderPending}
    onaddlink={addLink}
    onaddfolder={(path: string) =>
      addFolder(path).then((placed) => {
        if (placed) panel = null;
        return placed;
      })}
    onchoosefolder={() => void chooseFolder()}
    >{#snippet picker()}<WorkTabPicker
        {tabs}
        {openTabs}
        onopentabs={(on: boolean) => (openTabs = on)}
        {spaceName}
        {currentTabId}
        attachedTabIds={attachedTabs}
        pending={busy}
        onattach={(ids) => void attach(ids)}
        onopen={(id) => openPane({ kind: "tab", id }, null)}
        {onnewtab}
      />{/snippet}{#snippet media(kind: "document" | "image")}<WorkMediaPicker
        profile={session.profile}
        {kind}
        attachedIds={snapshot?.elements.flatMap((element) =>
          element.reference.kind === "resource" ? [element.reference.resource] : [],
        ) ?? []}
        pending={busy}
        onattach={(ids) => void attachResources(ids)}
      />{/snippet}</AttachPanel
  >{/snippet}
{#snippet barTools()}
  <BarTool
    icon={Cursor01Icon}
    label={m.work_tool_select()}
    keys={["V"]}
    pressed={pointerTool() === "select"}
    onclick={() => setPointerTool("select")}
  />
  <BarTool
    icon={HandIcon}
    label={m.work_tool_hand()}
    keys={["H"]}
    pressed={pointerTool() === "hand"}
    onclick={() => setPointerTool("hand")}
  />
  <span class="tool-rule" aria-hidden="true"></span>
  <BarTool
    icon={StickyNote03Icon}
    label={m.work_tool_note()}
    keys={["N"]}
    disabled={!snapshot}
    open={panel === "note"}
    onopenchange={(open) => openPanel("note", open)}
    >{#snippet content()}{#if notes}<NotePanel
          {notes}
          placed={snapshot?.elements.flatMap((element) =>
            element.reference.kind === "resource" ? [element.reference.resource] : [],
          ) ?? []}
          {busy}
          oncreate={() => {
            panel = null;
            void createNote();
          }}
          onpick={(id) => void placeNote(id)}
        />{/if}{/snippet}</BarTool
  >
  <BarTool
    icon={PlusSignIcon}
    label={m.work_tool_attach()}
    keys={["A"]}
    disabled={!snapshot}
    wide
    open={panel === "media"}
    onopenchange={(open) => openPanel("media", open)}
    content={attachPanel}
  />
{/snippet}
{#snippet barAttach()}<BarTool
    icon={PlusSignIcon}
    label={m.work_tool_attach()}
    small
    wide
    disabled={!snapshot}
    open={panel === "attach"}
    onopenchange={(open) => openPanel("attach", open)}
    content={attachPanel}
  />{/snippet}
{#snippet barAbove()}
  <!-- An open grant question stands over the bar, in the line's place, until it is answered. -->
  {#if grantOpen}<AccountGrantReview session={objectiveSession!} />{/if}
  {#if composerFailure}<p class="composer-alert" role="alert">
      {m.work_account_update_invalid()}
    </p>{/if}
{/snippet}
{#snippet agentLine()}
  <LazyView
    loader={loadAgentLine}
    loadingLabel=""
    failureLabel={m.surface_render_failed()}
    retryLabel={m.surface_retry()}
    >{#snippet children(Line)}
      <Line
        session={objectiveSession!}
        agents={agents.items}
        draft={session.composer}
        onreview={(step: string) => {
          panel = null;
          inspected = null;
          liftFile = null;
          lifted = { id: "", origin: null, proposal: step };
        }}
        waiting={agentWaiting}
        problem={workTasks.failed
          ? m.work_tasks_failed()
          : workNotes.failed
            ? m.work_note_failed()
            : null}
        ondismissproblem={() => {
          workTasks.dismiss();
          workNotes.dismiss();
        }}
        {writeup}
        onwaitingpage={(card: string) => {
          panel = null;
          canvasRef?.focusCard(pageEntries.get(card)?.part ?? card);
        }}
        onfocusagent={(id) => canvasRef?.center(id)}
        onsteered={() => (session.composer = "")}
        onsignin={(card: string) => void signIn(card)}
        retry={signInRetry?.work === objectiveSession?.selected ? signInRetry : null}
        onretry={() => void retrySignedIn()}
        onopenpage={(tab) => {
          if (!tabs.some((candidate) => candidate.id === tab)) return;
          const origin = snapshot?.elements.find(
            (element) => element.reference.kind === "browser" && element.reference.tab === tab,
          );
          openPane({ kind: "tab", id: tab }, origin?.id ?? null);
        }}
      />{/snippet}</LazyView
  >
{/snippet}
{#snippet composerContext()}
  {#if openTabs}<OpenTabsChip
      disabled={objectivePending || !!session.objectiveSubmission}
      onremove={() => (openTabs = false)}
    />{/if}
  {#if session.accountScope}
    <AccountScopeChip
      title={session.accountScope.title}
      origin={session.accountScope.origin}
      mode={session.accountScope.mode}
      bind:effect={accountEffect}
      disabled={objectivePending || !!session.objectiveSubmission}
      onremove={() => {
        clearAccount();
        composerFailure = null;
      }}
    />
  {:else if contextSel?.items.length}
    <ContextManifest profile={session.profile} selection={contextSel} purpose="agent" />
  {/if}
{/snippet}
<svelte:window onbeforeunload={abandonTakeover} onkeydown={toolKeys} />
<div class="environment">
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
              author={profileLabel}
              {pictures}
              initialView={canvasView}
              {remoteView}
              {authoritative}
              expose={(api) => (canvasRef = api)}
              board={boardActions}
              work={(objective: string) =>
                objectiveSession?.projection?.work.id === objective
                  ? objectiveSession.projection
                  : context.objectives.get(objective)}
              onmoved={(ids: string[]) => {
                for (const id of ids) moved.add(id);
              }}
              onprobe={(origin: string) =>
                void commands.faviconProbe(session.profile, [origin]).catch(() => false)}
              fitBottomInset={composerHeight}
              fitTopInset={56}
              still={!!lifted || !!pane || !!takeover}
              oninspect={(id: string) => (inspected = id)}
              onopen={openLift}
              onopenlink={openCitation}
              onselectionchange={(ids: string[]) => {
                selectedIds = ids.filter((id) => authoritative.has(id));
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
                if (action === "expand-request") {
                  if (openRequests.has(id)) openRequests.delete(id);
                  else openRequests.add(id);
                  return;
                }
                if (action?.startsWith("page:")) {
                  openBranchPage(action.slice("page:".length));
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
                // One page as the person: read it, or change one field and restore it.
                if (action === "account" || action === "account-update") {
                  const item = items.find((item) => item.id === id);
                  if (item?.type === "tab" && !item.unavailable && item.detail) {
                    session.accountScope = {
                      element: id,
                      title: item.title,
                      origin: item.detail,
                      mode: "page",
                    };
                    accountEffect =
                      action === "account"
                        ? { kind: "read" }
                        : { kind: "update", update: { field: null, from: "", to: "" } };
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
            <p>{aiEnabled ? m.work_env_manual_hint() : m.work_env_ai_off_hint()}</p>
          </div>{/if}
      {/key}{:else}<div class="welcome">
        <p>{session.loading ? m.surface_loading() : m.work_env_preparing()}</p>
      </div>{/if}
    <!-- The canvas never ends in a hard edge: it fades under what sits at its
         top and bottom, and costs nothing while nothing moves. -->
    <div class="edge-scrim top" aria-hidden="true"></div>
    <div class="edge-scrim bottom" aria-hidden="true"></div>
    <div class="canvas-top">
      <WorksMenu {session} untitled={m.work_env_default_title()} live={!!runStatus} />
      <span class="canvas-top-gap"></span>
      {#if runningObjective && objectiveSession && !grantOpen}<div class="island">
          {@render agentLine()}
        </div>{/if}
      {#if troubled}<div class="status" role="status">
          <span
            >{session.failure
              ? m.work_request_failed()
              : folderRefused
                ? m.work_env_folder_refused()
                : m.work_env_view_unsaved()}</span
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
      <AccountButton name={profileLabel} />
    </div>
    <div class="zoom-slot" data-work-zoom-slot></div>
  </div>
  {#if lifted && cardBounds}
    <Lift
      bind:this={liftRef}
      origin={lifted.origin}
      source={lifted.id || null}
      bounds={cardBounds}
      preferred={liftSize(liftedItem)}
      title={lifted.proposal ? m.work_line_review() : (liftedItem?.title ?? "")}
      onclose={() => {
        lifted = null;
        liftSource = null;
        liftFile = null;
        liftRecord = null;
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
      {:else if liftedCommand}
        <div class="lift-body">
          <LiftHeader
            kind={m.work_env_command()}
            title={liftedCommand.command.command}
            icon={ComputerTerminal01Icon}
          />
          {#await import("./local/CommandRecord.svelte") then module}
            <module.default record={liftedCommand} />
          {/await}
        </div>
      {:else if liftedItem?.sources}
        <div class="lift-body">
          <LiftHeader
            kind={liftedItem.type === "part" ? m.work_part() : m.work_sources()}
            title={liftedItem.title}
            meta={liftedItem.type === "part" && liftedItem.part?.helper === "browser"
              ? m.work_part_pages({ count: liftedItem.sources.length })
              : m.work_env_sources_count({ count: liftedItem.sources.length })}
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
              pages={recordedPages(subject.objective)}
              note={(markdown: string) => subjectNote(subject, markdown)}
              onask={askAbout}
              onopen={openCitation}
              onfile={(record: string) =>
                (liftFile = fileEvidence(context.objectives, record) ?? null)}
            />{/snippet}</LazyView
        >
      {:else if !liftedAgent && liftedElement?.reference.kind === "objective" && objectiveSession}
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
      {:else if liftedStage}
        <RunView
          request={liftedStage.request}
          trail={liftedStage.trail}
          runs={context.objectives
            .get(liftedStage.objective)
            ?.executions.filter((run) => liftedStage.executions.includes(run.id)) ?? []}
          made={liftedStage.board.blocks.flatMap((block) =>
            block.title ? [{ id: block.id, title: block.title }] : [],
          )}
          onsource={(row) => {
            if (row.file) {
              liftFile = fileEvidence(context.objectives, row.file.record) ?? null;
              return;
            }
            lifted = null;
            openPane({ kind: "url", url: row.url }, null);
          }}
          onmade={(id) => {
            lifted = null;
            canvasRef?.focusCard(id);
          }}
        />
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
                  primary={resultAction(liftedItem!.id)}
                  secondary={workTasks.state(liftedItem!.id) === "none"
                    ? undefined
                    : noteAction(liftedItem!.id)}
                  {pictures}
                  onopen={openCitation}
                  onfile={(record: string) =>
                    (liftFile = fileEvidence(context.objectives, record) ?? null)}
                />{/snippet}</LazyView
            >
          {:else}<LiftHeader kind={liftedItem.kind} title={liftedItem.title} />{/if}
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
      account={pane.account}
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
  <div class="bar-dock">
    {#if aiEnabled}<WorkBar
        bind:ref={composerElement}
        bind:value={session.composer}
        placeholder={lineRunning
          ? m.work_composer_steer()
          : runningObjective
            ? m.work_composer_continue()
            : m.work_composer_start()}
        disabled={objectivePending || !!session.objectiveSubmission}
        {busy}
        holding={panel === "attach"}
        {model}
        tools={barTools}
        attach={barAttach}
        above={grantOpen || composerFailure ? barAbove : undefined}
        context={openTabs || contextSel || session.accountScope ? composerContext : undefined}
        onsubmit={() => void send()}
      />{:else}<div class="tools-only" role="toolbar" aria-label={m.work_env_toolbar()}>
        {@render barTools()}
      </div>{/if}
  </div>
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

  /* Framed like a Browse page, 8px off every window edge; the bar below it
     stands on the window's own bottom edge, across that gap. */
  .canvas-card {
    position: absolute;
    inset: 0 0 8px;
    border-radius: var(--content-radius);
    background: var(--color-canvas);
    box-shadow: inset 0 0 0 1px var(--color-border);
    overflow: hidden;
  }

  /* The canvas keeps its cards' stacking to itself, so the edges can lie over them. */
  .canvas-card :global(.work-canvas) {
    isolation: isolate;
  }

  /* The canvas's edge fades to its own ground under the title and the bar. */
  .edge-scrim {
    position: absolute;
    inset-inline: 0;
    z-index: 4;
    block-size: 72px;
    pointer-events: none;
  }

  .edge-scrim.top {
    inset-block-start: 0;
    background: linear-gradient(to bottom, var(--color-canvas), transparent);
  }

  .edge-scrim.bottom {
    inset-block-end: 0;
    background: linear-gradient(to top, var(--color-canvas), transparent);
  }

  .canvas-top {
    position: absolute;
    inset-block-start: 12px;
    inset-inline: 12px 16px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 16px;
    min-inline-size: 0;
    pointer-events: none;
  }

  .canvas-top-gap {
    flex: 1;
  }

  .tool-rule {
    flex: none;
    inline-size: 1px;
    block-size: 18px;
    margin-inline: 4px;
    background: var(--color-border);
  }

  /* The run's line stands at the top centre, where the eye goes first, and
     opens downward over the canvas. */
  .island {
    position: absolute;
    inset-block-start: -2px;
    inset-inline-start: 50%;
    inline-size: max-content;
    min-inline-size: 240px;
    max-inline-size: min(520px, calc(100% - 560px));
    translate: -50% 0;
    pointer-events: auto;
    animation: island-in var(--motion-slow) var(--ease-spring) backwards;
  }

  @keyframes island-in {
    from {
      opacity: 0;
      scale: 0.9;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .island {
      animation: none;
    }
  }

  .zoom-slot {
    position: absolute;
    inset-block-end: 12px;
    inset-inline-end: 12px;
    z-index: 5;
    display: flex;
  }

  .bar-dock {
    position: absolute;
    inset-block-end: 0;
    inset-inline: 0;
    z-index: 30;
    display: flex;
    justify-content: center;
    pointer-events: none;
  }

  /* With AI off the bar is its tools alone. */
  .tools-only {
    display: flex;
    gap: 2px;
    padding: 8px;
    border-radius: var(--radius-panel) var(--radius-panel) 0 0;
    background: var(--color-menu);
    backdrop-filter: blur(14px) saturate(1.2);
    box-shadow: var(--shadow-popover);
    pointer-events: auto;
  }

  .welcome {
    position: absolute;
    inset: 36% 24px auto;
    pointer-events: none;
    text-align: center;
  }

  .welcome p {
    margin: 0;
    color: var(--color-muted);
  }

  .status {
    display: flex;
    align-items: center;
    gap: 10px;
    min-inline-size: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    pointer-events: auto;
  }

  .lift-body {
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-block-size: 0;
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
