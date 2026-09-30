<script lang="ts">
  import { untrack } from "svelte";
  import { taskSession } from "$domain/resources";
  import { loadTasks, type TaskScope } from "$features/tasks";
  import LazyView from "$shared/ui/LazyView";
  import * as m from "$shared/i18n/messages";

  /**
   * The person's tasks over the canvas, as the Tasks tool has them: a view
   * picked at the top (Today, Inbox, Upcoming or one of their lists), then
   * the Tasks feature's own list, composer and inline detail.
   */
  let { profile }: { profile: string } = $props();

  const session = $derived(untrack(() => taskSession(profile, "work")));
  $effect(() => {
    const current = session;
    void current.start("");
    return () => current.stop();
  });

  type View = { scope: TaskScope; list: string | null };
  let view = $state<View>({ scope: "today", list: null });
  const SCOPES: { scope: TaskScope; label: () => string }[] = [
    { scope: "today", label: m.task_scope_today },
    { scope: "inbox", label: m.task_scope_inbox },
    { scope: "upcoming", label: m.task_scope_upcoming },
  ];
  const lists = $derived(session.lists.filter((list) => !list.deleted));
</script>

<div class="tasks-panel">
  <div class="views" role="tablist" aria-label={m.panel_tasks()}>
    {#each SCOPES as entry (entry.scope)}<button
        type="button"
        role="tab"
        class="view"
        aria-selected={!view.list && view.scope === entry.scope}
        onclick={() => (view = { scope: entry.scope, list: null })}>{entry.label()}</button
      >{/each}
    {#if lists.length}<span class="rule" aria-hidden="true"></span>{/if}
    {#each lists as list (list.id)}<button
        type="button"
        role="tab"
        class="view"
        aria-selected={view.list === list.id}
        onclick={() => (view = { scope: "all", list: list.id })}>{list.title}</button
      >{/each}
  </div>
  <div class="body">
    <LazyView
      loader={loadTasks}
      loadingLabel={m.panel_loading()}
      failureLabel={m.panel_load_failed()}
      retryLabel={m.panel_retry()}
      >{#snippet children(Tasks)}<Tasks
          {session}
          scope={view.scope}
          listId={view.list}
          compact
        />{/snippet}</LazyView
    >
  </div>
</div>

<style>
  .tasks-panel {
    display: flex;
    flex-direction: column;
    gap: 6px;
    block-size: min(520px, calc(100vh - 160px));
    min-block-size: 0;
  }

  /* The views as quiet tabs, the lists after a hairline, scrolling sideways when many. */
  .views {
    display: flex;
    flex: none;
    align-items: center;
    gap: 2px;
    padding: 2px 2px 6px;
    overflow-x: auto;
    border-block-end: 1px solid var(--color-border);
    scrollbar-width: none;
  }

  .view {
    flex: none;
    block-size: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    white-space: nowrap;
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .view:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .view[aria-selected="true"] {
    background: var(--color-fill-active);
    color: var(--color-text);
  }

  .view:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .rule {
    flex: none;
    inline-size: 1px;
    block-size: 14px;
    margin-inline: 6px;
    background: var(--color-border);
  }

  .body {
    flex: 1;
    min-block-size: 0;
    overflow: auto;
  }
</style>
