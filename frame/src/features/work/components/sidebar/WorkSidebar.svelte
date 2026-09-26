<script lang="ts">
  import { tick, untrack } from "svelte";
  import type { WorkEnvironmentSummary } from "$shared/ipc/bindings";
  import { environmentSession } from "$domain/work-environment";
  import { captureFrom, playTo } from "$shared/lib/plate-glide";
  import { ListMotion } from "$shared/lib/list-motion";
  import { rememberScroll } from "$shared/ui/scroll-memory";
  import Icon from "$shared/ui/Icon";
  import { Add01Icon, Archive01Icon, ArrowRight01Icon } from "../../lib/icons";
  import ProjectMark from "./ProjectMark.svelte";
  import ProjectMenu from "./ProjectMenu.svelte";
  import * as m from "$shared/i18n/messages";

  let { profile, space, compact }: { profile: string; space: string; compact: boolean } = $props();

  const session = untrack(() => environmentSession(profile, space));
  const works = $derived(session?.works ?? []);
  const active = $derived(works.filter((work) => work.lifecycle === "active"));
  const archived = $derived(works.filter((work) => work.lifecycle === "archived"));
  const current = $derived(session?.snapshot?.id ?? session?.selected ?? null);
  const busy = $derived(!!session?.pending || !!session?.loading);
  let showArchived = $state(false);
  let renaming = $state<string | null>(null);
  let draft = $state("");
  let list = $state<HTMLElement>();

  function plateOf(id: string | null) {
    const host = id
      ? document.querySelector<HTMLElement>(`[data-work-project="${CSS.escape(id)}"]`)
      : null;
    return host?.matches("[data-plate]")
      ? host
      : (host?.querySelector<HTMLElement>("[data-plate]") ?? null);
  }
  // The current project's plate travels to the next one, as a tab's does.
  let shown: string | null = null;
  let glide: ReturnType<typeof captureFrom> = null;
  $effect.pre(() => {
    const next = current;
    untrack(() => {
      if (next !== shown) glide = captureFrom(plateOf(shown));
    });
  });
  $effect(() => {
    const next = current;
    untrack(() => {
      if (next === shown) return;
      playTo(glide, plateOf(next));
      glide = null;
      shown = next;
    });
  });

  const motion = new ListMotion({ enter: () => "rise" });
  const shape = $derived(active.map((work) => work.id).join(" "));
  $effect.pre(() => {
    void shape;
    untrack(() => motion.capture(list));
  });
  $effect(() => {
    void shape;
    untrack(() => motion.play(list));
  });

  function open(work: WorkEnvironmentSummary) {
    if (!session || work.id === current || renaming === work.id) return;
    void session.open(work.id);
  }

  async function create() {
    if (!session || busy) return;
    await session.create(m.work_env_default_title());
  }

  async function beginRename(work: WorkEnvironmentSummary) {
    renaming = work.id;
    draft = work.title;
    await tick();
    const field = list?.querySelector<HTMLInputElement>("[data-project-rename]");
    field?.focus();
    field?.select();
  }

  async function commitRename(work: WorkEnvironmentSummary) {
    if (renaming !== work.id) return;
    renaming = null;
    const title = draft.trim();
    if (!session || !title || title === work.title) return;
    await session.editWork(work.id, { kind: "rename", title });
  }

  async function archive(work: WorkEnvironmentSummary) {
    if (!session) return;
    const next = active.find((candidate) => candidate.id !== work.id);
    if (!(await session.editWork(work.id, { kind: "set_lifecycle", lifecycle: "archived" })))
      return;
    if (next) await session.open(next.id);
    else await session.create(m.work_env_default_title());
  }

  function restore(work: WorkEnvironmentSummary) {
    void session?.editWork(work.id, { kind: "set_lifecycle", lifecycle: "active" });
  }

  function renameKey(event: KeyboardEvent, work: WorkEnvironmentSummary) {
    if (event.key === "Enter") {
      event.preventDefault();
      void commitRename(work);
    } else if (event.key === "Escape") {
      event.preventDefault();
      renaming = null;
    }
  }
</script>

{#snippet row(work: WorkEnvironmentSummary, index: number)}
  {@const selected = work.id === current}
  {@const live = session?.running === work.id}
  <li
    class="project"
    data-motion-key={`project:${work.id}`}
    data-work-project={work.id}
    data-plate
    data-selected={selected}
    data-archived={work.lifecycle === "archived"}
    data-cascade
    style:--cascade={index}
  >
    {#if renaming === work.id}
      <div class="project-open">
        <ProjectMark title={draft || work.title} current={selected} />
        <input
          class="project-field"
          data-project-rename
          aria-label={m.work_project_title()}
          maxlength="128"
          bind:value={draft}
          onkeydown={(event) => renameKey(event, work)}
          onblur={() => void commitRename(work)}
        />
      </div>
    {:else}
      <ProjectMenu
        archived={work.lifecycle === "archived"}
        disabled={busy}
        onrename={() => void beginRename(work)}
        onarchive={() => void archive(work)}
        onrestore={() => restore(work)}
      >
        <button
          type="button"
          class="project-open"
          aria-current={selected ? "page" : undefined}
          aria-label={live ? m.work_project_running({ title: work.title }) : work.title}
          disabled={busy && !selected}
          onclick={() => open(work)}
          ondblclick={() => void beginRename(work)}
        >
          <ProjectMark title={work.title} current={selected} />
          <span class="project-label">{work.title}</span>
          {#if live}<span class="live" aria-hidden="true"></span>{/if}
        </button>
      </ProjectMenu>
    {/if}
  </li>
{/snippet}

{#if session}
  {#if compact}
    <div
      use:rememberScroll={`${profile}/${space}/work-rail`}
      class="rail-scroller"
      data-glide-scroller
    >
      <ul bind:this={list} class="rail-list" aria-label={m.work_projects()}>
        {#each active as work, index (work.id)}
          {@const selected = work.id === current}
          <li
            data-motion-key={`project:${work.id}`}
            data-work-project={work.id}
            data-cascade
            style:--cascade={index}
          >
            <ProjectMenu
              archived={false}
              disabled={busy}
              onarchive={() => void archive(work)}
              onrestore={() => restore(work)}
            >
              <button
                type="button"
                class="rail-item"
                data-plate
                title={work.title}
                aria-current={selected ? "page" : undefined}
                aria-label={session.running === work.id
                  ? m.work_project_running({ title: work.title })
                  : work.title}
                disabled={busy && !selected}
                onclick={() => open(work)}
              >
                <ProjectMark title={work.title} current={selected} />
                {#if session.running === work.id}<span class="live" aria-hidden="true"></span>{/if}
              </button>
            </ProjectMenu>
          </li>
        {/each}
        <li data-cascade style:--cascade={active.length}>
          <button
            type="button"
            class="rail-item rail-new"
            title={m.work_project_new()}
            aria-label={m.work_project_new()}
            disabled={busy}
            onclick={() => void create()}
          >
            <Icon icon={Add01Icon} size={16} />
          </button>
        </li>
      </ul>
    </div>
  {:else}
    <div
      use:rememberScroll={`${profile}/${space}/work-projects`}
      class="project-scroller"
      data-glide-scroller
    >
      <div class="new-project-slot" data-cascade style:--cascade={0}>
        <button type="button" class="new-project" disabled={busy} onclick={() => void create()}>
          <Icon icon={Add01Icon} size={16} />
          <span>{m.work_project_new()}</span>
        </button>
      </div>
      <ul bind:this={list} class="project-list" aria-label={m.work_projects()}>
        {#each active as work, index (work.id)}{@render row(work, index + 1)}{/each}
      </ul>
      {#if session.next && works.length < 256}<button
          type="button"
          class="project-more"
          onclick={() => void session.reload(true)}>{m.resource_more()}</button
        >{/if}
      {#if archived.length}
        <div class="archive">
          <button
            type="button"
            class="folder-row"
            aria-expanded={showArchived}
            onclick={() => (showArchived = !showArchived)}
          >
            <Icon icon={Archive01Icon} size={15} /><span>{m.work_archived()}</span><span
              class="folder-chevron"
              data-expanded={showArchived}><Icon icon={ArrowRight01Icon} size={12} /></span
            >
          </button>
          {#if showArchived}<ul class="project-list" aria-label={m.work_archived()}>
              {#each archived as work, index (work.id)}{@render row(work, index)}{/each}
            </ul>{/if}
        </div>
      {/if}
    </div>
  {/if}
{/if}

<style>
  .project-scroller {
    min-height: 0;
    flex: 1;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding-block-end: 4px;
  }

  .new-project-slot {
    padding-inline: var(--sidebar-inset);
    padding-block: 2px calc(var(--sidebar-row-gap) + 2px);
  }

  .new-project,
  .project-more {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: var(--row-sidebar);
    padding-inline: 8px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-faint);
    font: inherit;
    font-size: var(--sidebar-row-text);
    font-weight: var(--sidebar-row-weight);
    letter-spacing: -0.005em;
    text-align: start;
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .new-project:hover:not(:disabled),
  .project-more:hover {
    background: var(--row-hover);
    color: var(--color-label-secondary);
  }

  .project-list {
    display: flex;
    flex-direction: column;
    gap: var(--sidebar-row-gap);
    margin: 0;
    padding: 0 var(--sidebar-inset);
    list-style: none;
  }

  /* A tab row, drawn for a project: the same height, corner, fills and rim. */
  .project {
    position: relative;
    display: flex;
    align-items: center;
    height: var(--row-sidebar);
    border-radius: var(--radius-row);
    color: var(--color-label-secondary);
    font-size: var(--sidebar-row-text);
    font-weight: var(--sidebar-row-weight);
    letter-spacing: -0.005em;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out),
      color var(--motion-base) var(--ease-out);
  }

  .project:not([data-selected="true"]):hover {
    background: var(--row-hover);
    color: var(--color-text);
  }

  .project[data-selected="true"] {
    background: var(--row-active);
    box-shadow: var(--row-rim);
    color: var(--color-text);
    font-weight: var(--sidebar-row-weight-current);
  }

  .project[data-archived="true"]:not([data-selected="true"]) {
    color: var(--color-muted);
  }

  .project-open {
    display: flex;
    flex: 1;
    align-items: center;
    align-self: stretch;
    gap: 10px;
    min-width: 0;
    padding-inline: 7px 10px;
    border: 0;
    border-radius: inherit;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .project-open:focus-visible {
    outline-offset: -2px;
  }

  .project-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    mask-image: var(--mask-fade-end);
  }

  .project-label:dir(rtl) {
    mask-image: var(--mask-fade-end-rtl);
  }

  .project-field {
    flex: 1;
    min-width: 0;
    height: 24px;
    margin-inline-start: -4px;
    padding: 0 4px;
    border: 0;
    border-radius: var(--radius-inset);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    outline: none;
    box-shadow: var(--shadow-field-focus);
  }

  /* Its run is live: one still mark, never a loop. */
  .live {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-lit) 18%, transparent);
  }

  .archive {
    margin-block-start: 10px;
    list-style: none;
    padding-inline: var(--sidebar-inset);
  }

  .archive .folder-row {
    padding-inline-start: 8px;
  }

  .archive .project-list {
    margin-block-start: var(--sidebar-row-gap);
    padding: 0;
  }

  .rail-scroller {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding-block: 8px;
  }

  .rail-list {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sidebar-row-gap);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .rail-item {
    position: relative;
    display: grid;
    place-items: center;
    width: 40px;
    height: var(--row-sidebar);
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-faint);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      scale var(--motion-slow) var(--ease-spring);
  }

  .rail-item:hover:not(:disabled) {
    background: var(--row-hover);
    color: var(--color-label-secondary);
  }

  .rail-item:active:not(:disabled) {
    scale: 0.94;
    transition-duration: var(--motion-instant);
  }

  .rail-item[aria-current="page"] {
    background: var(--row-active);
    box-shadow: var(--row-rim);
  }

  .rail-item .live {
    position: absolute;
    inset-block-start: 4px;
    inset-inline-end: 3px;
  }
</style>
