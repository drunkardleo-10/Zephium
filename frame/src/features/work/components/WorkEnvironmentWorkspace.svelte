<script lang="ts">
  import { untrack, onMount } from "svelte";
  import { WorkEnvironmentContext, type WorkEnvironmentSession } from "$domain/work-environment";
  import { commandId, workSession, type WorkSession } from "$domain/work";
  import { resourceSession, type ResourceSession } from "$domain/resources";
  import type { TabView } from "$shared/ipc/bindings";
  import { preferences } from "$domain/preferences";
  import { loadNotes } from "$features/notes";
  import Button from "$shared/ui/Button";
  import Checkbox from "$shared/ui/Checkbox";
  import { publicResearchQueryValid } from "../lib/public-research";
  import LazyView from "$shared/ui/LazyView";
  import WorkEnvironment from "./WorkEnvironment.svelte";
  import WorkTabPicker from "./WorkTabPicker.svelte";
  import { environmentPlan } from "../lib/project-environment-plan";
  import { environmentItems, environmentView } from "../lib/project-environment";
  import { environmentResults, type ResultReference } from "../lib/project-environment-results";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import type { CanvasView } from "../lib/canvas-model";
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
  const id = $props.id();
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
  let objectiveSession = $state.raw<WorkSession | null>(null);
  let inspected = $state<string | null>(null);
  let objectiveOpen = $state(false);
  let objectivePanel = $state<HTMLElement | undefined>();
  $effect(() => {
    if (objectiveOpen && objectivePanel) {
      inspected = null;
      objectivePanel.focus();
    }
  });
  let notesOpen = $state(false);
  let title = $state("");
  let areaTitle = $state("");
  let objectivePending = $state(false);
  let composerFailure = $state<"limit" | "changed" | null>(null);
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
  let archived = $state(false);
  let list = $state(false);
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
  const loadArtifact = () => import("$shared/ui/data/Artifact");
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
        // A reconciled environment reply may already contain this exact reference.
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
      // Once admitted, this operation owns reconciliation; composer submission cannot replay it.
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
  <div class="panel-stack">
    <form
      onsubmit={(event) => {
        event.preventDefault();
        if (title.trim())
          void session.create(title.trim()).then((okay) => {
            if (okay) title = "";
          });
      }}
    >
      <label for={`${id}-title`}>{m.work_env_new_work()}</label><input
        id={`${id}-title`}
        bind:value={title}
        maxlength="128"
        placeholder={m.work_env_work_title()}
        disabled={busy}
      /><Button type="submit" disabled={busy || !title.trim()}>{m.work_env_create()}</Button>
    </form>
    <Button size="compact" aria-pressed={archived} onclick={() => (archived = !archived)}
      >{m.work_archived()}</Button
    >
    <ul>
      {#each session.works.filter((work) => (work.lifecycle === "archived") === archived) as work (work.id)}<li
        >
          <Button
            size="compact"
            disabled={busy}
            aria-pressed={snapshot?.id === work.id}
            onclick={() => {
              inspected = null;
              void session.open(work.id);
            }}>{work.title}</Button
          >
        </li>{/each}
    </ul>
    {#if session.next && session.works.length < 256}<Button
        size="compact"
        disabled={busy}
        onclick={() => void session.reload(true)}>{m.resource_more()}</Button
      >{/if}
    {#if snapshot}<Button
        size="compact"
        disabled={busy}
        onclick={() =>
          void session.edit({
            kind: "set_lifecycle",
            lifecycle: snapshot.lifecycle === "active" ? "archived" : "active",
          })}
        >{snapshot.lifecycle === "active" ? m.work_env_archive() : m.work_env_restore()}</Button
      >{/if}
  </div>
{/snippet}
{#snippet createPanel()}
  <div class="panel-stack">
    <form
      onsubmit={(event) => {
        event.preventDefault();
        if (areaTitle.trim())
          void session.edit({ kind: "create_area", title: areaTitle.trim() }).then((okay) => {
            if (okay) areaTitle = "";
          });
      }}
    >
      <label for={`${id}-area`}>{m.work_env_new_area()}</label><input
        id={`${id}-area`}
        bind:value={areaTitle}
        maxlength="128"
        disabled={busy}
      /><Button type="submit" disabled={busy || !areaTitle.trim()}>{m.work_env_create()}</Button>
    </form>
    <Button size="compact" aria-pressed={list} onclick={() => (list = !list)}
      >{list ? m.work_canvas_label() : m.work_env_list()}</Button
    >
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
  <Button
    size="compact"
    disabled={busy ||
      !notes?.record ||
      snapshot?.elements.some(
        (element) =>
          element.reference.kind === "resource" && element.reference.resource === notes?.record?.id,
      )}
    onclick={() => void attachNote()}>{m.work_env_attach_note()}</Button
  >
{/snippet}
{#snippet profilePanel()}<div class="panel-stack">
    <strong>{profileLabel}</strong>
    <p>{m.work_env_local_profile()}</p>
    <label class="toggle"
      ><input
        type="checkbox"
        checked={preferences.value("ai.enabled") !== "false"}
        disabled={preferences.saving()}
        onchange={(event) =>
          void preferences.set("ai.enabled", String(event.currentTarget.checked))}
      />{m.work_env_ai_enabled()}</label
    >
    <label class="toggle"
      ><input
        type="checkbox"
        checked={preferences.value("work.enabled") !== "false"}
        disabled={preferences.saving()}
        onchange={(event) =>
          void preferences.set("work.enabled", String(event.currentTarget.checked))}
      />{m.work_env_work_enabled()}</label
    >
    {#if preferences.saveFailed()}<p role="status">{m.work_env_setting_failed()}</p>{/if}
    <Button onclick={onsettings}>{m.work_env_settings()}</Button>
  </div>{/snippet}
{#snippet taskPanel()}
  <div class="panel-stack">
    <strong>{m.work_env_objectives()}</strong>
    <ul>
      {#each objectiveSession?.works ?? [] as work (work.id)}{@const attached =
          snapshot?.elements.some(
            (element) =>
              element.reference.kind === "objective" && element.reference.objective === work.id,
          )}
        <li>
          <strong>{work.objective}</strong>
          <div class="objective-actions">
            <Button
              size="compact"
              onclick={() => {
                if (attached) void continueObjective(work.id);
                else
                  void objectiveSession?.open(work.id).then((okay) => {
                    if (okay) {
                      inspectionExecution = null;
                      inspectCurrentPlan = false;
                      objectiveOpen = true;
                    }
                  });
              }}>{attached ? m.work_env_continue_work() : m.work_env_plan_details()}</Button
            >
            <Button
              size="compact"
              disabled={busy ||
                snapshot?.elements.some(
                  (element) =>
                    element.reference.kind === "objective" &&
                    element.reference.objective === work.id,
                )}
              onclick={() =>
                void session.edit({
                  kind: "add",
                  reference: { kind: "objective", objective: work.id },
                  area: null,
                })}>{m.work_env_attach_objective()}</Button
            >
          </div>
        </li>{:else}<li>{m.work_env_no_objectives()}</li>{/each}
    </ul>
    {#if objectiveSession?.next}<Button
        size="compact"
        onclick={() => void objectiveSession?.reload(true)}>{m.resource_more()}</Button
      >{/if}
    {#if objectiveSession?.failure}<p role="status">
        {m.work_request_failed({ reason: objectiveSession.failure })}
      </p>{/if}
  </div>
{/snippet}
{#snippet composer()}
  <div class="interaction-stack" bind:this={composerElement}>
    {#if !objectiveOpen && objectiveSession?.projection && snapshot?.elements.some((element) => element.reference.kind === "objective" && element.reference.objective === objectiveSession?.selected)}<div
        class="interaction-scroll"
      >
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
      </div>{/if}
    <form
      class="input-panel"
      class:has-prompt={!!session.composer.trim()}
      class:research={session.publicResearch}
      onsubmit={(event) => {
        event.preventDefault();
        void createObjective();
      }}
    >
      <label class="sr-only" for={`${id}-objective`}>{m.work_env_prompt()}</label><textarea
        id={`${id}-objective`}
        rows="1"
        placeholder={m.work_env_prompt()}
        maxlength="8192"
        bind:value={session.composer}
        disabled={objectivePending || !!session.objectiveSubmission}></textarea><Button
        type="submit"
        size="compact"
        disabled={busy || !session.composer.trim()}
        >{session.publicResearch ? m.work_env_public_research() : m.work_env_start_work()}</Button
      >
      {#if objectiveSession?.failure}<p role="status">
          {m.work_request_failed({ reason: objectiveSession.failure })}
        </p>{/if}
      <div class="workflow-choice">
        <Checkbox
          label={m.work_env_public_research()}
          checked={session.publicResearch}
          disabled={objectivePending || !!session.objectiveSubmission}
          onchange={(checked) => {
            session.publicResearch = checked;
            composerFailure = null;
          }}
        />
      </div>
      {#if composerFailure}<p role="alert">
          {composerFailure === "limit"
            ? m.work_env_public_query_limit()
            : m.work_env_public_query_changed()}
        </p>{/if}
      <p class="disclosure">
        {session.publicResearch ? m.work_env_public_disclosure() : m.work_planning_disclosure()}
      </p>
    </form>
  </div>
{/snippet}
{#snippet canvas()}
  {#if snapshot}{#key `${snapshot.id}:${session.canvasRevision}`}
      {#if list}<div class="list-view">
          <ul>
            {#each items as item (item.id)}<li>
                <Button onclick={() => void inspectCanvas(item.id)}>{item.title}</Button>
                {#if item.artifact}<LazyView
                    loader={loadArtifact}
                    loadingLabel={m.surface_loading()}
                    failureLabel={m.surface_render_failed()}
                    retryLabel={m.surface_retry()}
                    >{#snippet children(Artifact)}<Artifact
                        artifact={item.artifact!}
                        onevidence={(source) => void inspectResult(item.id, source)}
                      />{/snippet}</LazyView
                  >{:else}<p>{item.detail}</p>{/if}
              </li>{/each}
          </ul>
        </div>
      {:else}<LazyView
          loader={loadCanvas}
          loadingLabel={m.surface_loading()}
          failureLabel={m.surface_render_failed()}
          retryLabel={m.surface_retry()}
          >{#snippet children(Canvas)}<Canvas
              {items}
              links={scene.links}
              initialView={canvasView}
              fitBottomInset={composerHeight}
              oninspect={(id: string) => void inspectCanvas(id)}
              onevidence={(id: string, source: EvidenceReference) => void inspectResult(id, source)}
              onaction={(id: string) => {
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
        >{/if}
      {#if snapshot.elements.length === 0}<div class="welcome">
          <h1>{snapshot.title}</h1>
          <p>{m.work_env_empty_hint()}</p>
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
  <div class="results-disclosure">
    {#if results.remaining}<Button
        size="compact"
        onclick={() => {
          if (!results.remaining) return;
          inspectionExecution = results.remaining.execution;
          inspectCurrentPlan = false;
          objectiveOpen = true;
        }}>{m.work_env_other_results({ count: results.remaining.count })}</Button
      >{/if}
  </div>
  {#if element && item && !objectiveOpen && !notesOpen && !resultSelection}<section
      class="inspector"
      aria-label={m.work_env_selection()}
    >
      <div class="panel-stack">
        <Button size="compact" onclick={() => (inspected = null)}>{m.work_env_close()}</Button>
        <h2>{item.title}</h2>
        <p>{item.detail}</p>
        <p>{item.status}</p>
        <Button
          disabled={busy ||
            (element.reference.kind === "browser" &&
              !tabs.some(
                (tab) => element.reference.kind === "browser" && tab.id === element.reference.tab,
              ))}
          onclick={() => void inspectReference()}
          >{element.reference.kind === "browser"
            ? m.work_env_open_browse()
            : m.work_env_open_resource()}</Button
        >
        {#if element.reference.kind === "objective"}<Button
            onclick={() => {
              if (element?.reference.kind === "objective")
                void continueObjective(element.reference.objective);
            }}>{m.work_env_continue_work()}</Button
          >{/if}
        <label for={`${id}-group`}>{m.work_env_area()}</label><select
          id={`${id}-group`}
          disabled={busy}
          value={element.area ?? ""}
          onchange={(event) => {
            if (element)
              void session.edit({
                kind: "assign_area",
                element: element.id,
                area: event.currentTarget.value || null,
              });
          }}
          ><option value="">{m.work_env_no_area()}</option
          >{#each snapshot?.areas ?? [] as area (area.id)}<option value={area.id}
              >{area.title}</option
            >{/each}</select
        >
        <Button
          disabled={busy}
          onclick={() => {
            if (element)
              void session.edit({ kind: "remove", element: element.id }).then((okay) => {
                if (okay) inspected = null;
              });
          }}>{m.work_env_remove_reference()}</Button
        >
      </div>
    </section>{/if}
  {#if resultSelection && objectiveSession && !objectiveOpen && !notesOpen}<section
      class="inspector"
      aria-label={m.work_env_results()}
    >
      <Button size="compact" onclick={() => (resultSelection = null)}>{m.work_env_close()}</Button>
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
      <Button onclick={() => void closeNotes()}>{m.work_env_close()}</Button>{@render notesPanel()}
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
{/snippet}
<WorkEnvironment
  {spaceName}
  workTitle={snapshot?.title ?? m.work_env_default_title()}
  {profileLabel}
  initialTabsOpen={!session.tabsIntroduced}
  panels={{
    tabs: tabPanel,
    switcher,
    create: createPanel,
    notes: notesPanel,
    profile: profilePanel,
    tasks: taskPanel,
  }}
  taskLabel={m.work_env_objective_count({
    count:
      snapshot?.elements.filter((element) => element.reference.kind === "objective").length ?? 0,
  })}
  children={canvas}
  composer={aiEnabled ? composer : undefined}
  {onreturn}
  onpanelchange={(panel) => {
    session.tabsIntroduced = true;
    if (panel === "notes") notesOpen = false;
    if (panel === "tasks") {
      objectiveSession = workSession(session.profile);
      void objectiveSession?.start();
    }
  }}
/>

<style>
  .panel-stack,
  form {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .toggle {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .objective-actions {
    display: flex;
    gap: 8px;
    margin-block-start: 8px;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    margin-block: 8px;
  }

  p,
  h2 {
    margin: 0;
    overflow-wrap: anywhere;
  }

  p {
    color: var(--color-muted);
  }

  input,
  textarea,
  select {
    box-sizing: border-box;
    inline-size: 100%;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    padding: 8px;
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
  }

  .toggle input {
    inline-size: auto;
  }

  input:focus-visible,
  textarea:focus-visible,
  select:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .notes-panel {
    display: flex;
    block-size: 440px;
    min-inline-size: 0;
    margin-block-end: 12px;
  }

  .welcome {
    position: absolute;
    inset: 35% 24px auto;
    pointer-events: none;
    text-align: center;
  }

  .welcome h1 {
    font-size: var(--text-title);
    font-weight: 500;
    margin-block: 0 12px;
  }

  .results-disclosure {
    position: absolute;
    inset-block-start: 88px;
    inset-inline-end: 16px;
  }

  .status {
    position: absolute;
    inset-inline-start: 16px;
    inset-block-end: 64px;
    max-inline-size: calc(100% - 32px);
    display: flex;
    gap: 12px;
    padding: 12px;
    border-radius: var(--radius-control);
    background: var(--color-surface);
    color: var(--color-muted);
  }

  .inspector,
  .detail {
    position: absolute;
    inset-block: 88px 80px;
    inset-inline-end: 16px;
    inline-size: min(340px, calc(100% - 32px));
    box-sizing: border-box;
    overflow: auto;
    padding: 16px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface);
    box-shadow: var(--shadow-popover);
  }

  .detail {
    inline-size: min(480px, calc(100% - 32px));
    z-index: 5;
  }

  .interaction-stack {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-block-size: min(60vh, 560px);
  }

  .interaction-scroll {
    overflow: auto;
    min-block-size: 0;
  }

  .input-panel {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    padding: 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface);
    box-shadow: var(--shadow-float);
  }

  .input-panel textarea {
    min-block-size: 32px;
    block-size: 32px;
    resize: none;
    margin: 0;
  }

  .input-panel:focus-within textarea,
  .input-panel.has-prompt textarea {
    block-size: 64px;
  }

  .input-panel > p {
    grid-column: 1 / -1;
  }

  .workflow-choice {
    display: none;
    grid-column: 1 / -1;
  }

  .input-panel:focus-within .workflow-choice,
  .input-panel.has-prompt .workflow-choice,
  .input-panel.research .workflow-choice {
    display: block;
  }

  .disclosure {
    display: none;
    margin: 0;
    font-size: var(--text-caption);
  }

  .input-panel:focus-within .disclosure,
  .input-panel.has-prompt .disclosure,
  .input-panel.research .disclosure {
    display: block;
  }

  .list-view {
    position: absolute;
    inset: 96px 24px;
    overflow: auto;
  }
</style>
