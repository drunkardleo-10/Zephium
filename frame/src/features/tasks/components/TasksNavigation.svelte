<script lang="ts">
  import { tick } from "svelte";
  import type { TaskSession } from "$domain/resources";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
  import Menu from "$shared/ui/Menu";
  import {
    Add01Icon,
    Calendar03Icon,
    Cancel01Icon,
    CheckListIcon,
    Delete02Icon,
    Folder01Icon,
    InboxIcon,
    MoreHorizontalIcon,
    Sun03Icon,
    Tick02Icon,
  } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import { pageView, setPageView } from "../lib/page-view.svelte";

  let { session, onclose }: { session: TaskSession; onclose: () => void } = $props();
  let view = $derived(pageView());
  let editing = $state<{ id: string | null; title: string } | null>(null);
  let field = $state<HTMLInputElement>();
  let busy = $state(false);
  const entries = $derived([
    {
      id: "inbox" as const,
      icon: InboxIcon,
      label: m.task_scope_inbox(),
      total: session.counts.inbox,
    },
    {
      id: "today" as const,
      icon: Sun03Icon,
      label: m.task_scope_today(),
      total: session.counts.today,
    },
    {
      id: "upcoming" as const,
      icon: Calendar03Icon,
      label: m.task_scope_upcoming(),
      total: session.counts.upcoming,
    },
    {
      id: "all" as const,
      icon: CheckListIcon,
      label: m.task_scope_all(),
      total: session.counts.all,
    },
  ]);
  async function begin(id: string | null = null, title = "") {
    editing = { id, title };
    await tick();
    field?.focus();
    field?.select();
  }
  async function save() {
    if (!editing?.title.trim() || busy) return;
    busy = true;
    const list = await session.saveList(editing.title, editing.id ?? undefined);
    if (list) {
      editing = null;
      setPageView({ scope: "all", list: list.id, trashed: false });
    }
    busy = false;
  }
</script>

<nav class="task-navigation" aria-label={m.tool_tasks()}>
  <header class="navigation-head">
    <span>{m.task_page_title()}</span><IconButton
      icon={Cancel01Icon}
      label={m.task_page_close()}
      onclick={onclose}
    />
  </header>
  <div class="navigation-group">
    {#each entries as entry (entry.id)}<button
        type="button"
        class="nav-row"
        aria-current={!view.trashed && !view.list && view.scope === entry.id ? "page" : undefined}
        onclick={() => setPageView({ scope: entry.id, list: null, trashed: false })}
        ><Icon icon={entry.icon} size={17} /><span>{entry.label}</span>{#if entry.total}<small
            >{entry.total}</small
          >{/if}</button
      >{/each}
  </div>
  <div class="navigation-lists">
    <div class="navigation-heading">
      <span>{m.task_lists()}</span><IconButton
        icon={Add01Icon}
        label={m.task_new_list()}
        onclick={() => void begin()}
      />
    </div>
    <div class="navigation-group">
      {#each session.lists as list (list.id)}<div class="list-row">
          <button
            class="nav-row"
            type="button"
            aria-current={!view.trashed && view.list === list.id ? "page" : undefined}
            onclick={() => setPageView({ scope: "all", list: list.id, trashed: false })}
            ><Icon icon={Folder01Icon} size={16} /><span>{list.title}</span>{#if list.count}<small
                >{list.count}</small
              >{/if}</button
          >
          <Menu
            label={m.task_list_actions({ title: list.title })}
            triggerClass="list-menu"
            entries={[
              { kind: "item", id: "rename", label: m.task_list_rename() },
              { kind: "separator" },
              { kind: "item", id: "delete", label: m.task_list_delete(), danger: true },
            ]}
            onselect={(id) => {
              if (id === "rename") void begin(list.id, list.title);
              else
                void session.deleteList(list.id).then((deleted) => {
                  if (deleted && view.list === list.id)
                    setPageView({ scope: "inbox", list: null, trashed: false });
                });
            }}>{#snippet trigger()}<Icon icon={MoreHorizontalIcon} size={14} />{/snippet}</Menu
          >
        </div>{/each}
      {#if editing}<form
          class="list-editor"
          onsubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <Icon icon={Folder01Icon} size={16} /><input
            bind:this={field}
            aria-label={m.task_list_name()}
            placeholder={m.task_list_name()}
            maxlength="64"
            bind:value={editing.title}
            disabled={busy}
            onkeydown={(event) => {
              if (event.key === "Escape") editing = null;
            }}
          /><IconButton
            icon={Tick02Icon}
            label={m.task_list_save()}
            disabled={busy || !editing.title.trim()}
            onclick={() => void save()}
          />
        </form>{:else if session.lists.length === 0}<button
          type="button"
          class="nav-row create-list"
          onclick={() => void begin()}
          ><Icon icon={Add01Icon} size={16} /><span>{m.task_new_list()}</span></button
        >{/if}
    </div>
  </div>
  <div class="navigation-secondary">
    <button
      type="button"
      class="nav-row"
      aria-current={!view.trashed && !view.list && view.scope === "completed" ? "page" : undefined}
      onclick={() => setPageView({ scope: "completed", list: null, trashed: false })}
      ><Icon icon={Tick02Icon} size={17} /><span>{m.task_scope_completed()}</span></button
    ><button
      type="button"
      class="nav-row"
      aria-current={view.trashed ? "page" : undefined}
      onclick={() => setPageView({ trashed: true, list: null })}
      ><Icon icon={Delete02Icon} size={16} /><span>{m.resource_show_trash()}</span></button
    >
  </div>
</nav>

<style>
  .task-navigation {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 0 10px 10px;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
  }

  .navigation-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex: none;
    height: 52px;
    padding-inline: 10px 0;
    color: var(--color-text);
    font-size: var(--text-body);
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  /* Lists take whatever height is left and scroll inside it, so Completed and
     Trash stay where the hand expects them: at the bottom. */
  .navigation-lists {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .navigation-group {
    display: grid;
    gap: 3px;
  }

  .nav-row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    width: 100%;
    height: 36px;
    padding-inline: 10px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-label-secondary);
    font: inherit;
    font-size: 13px;
    font-weight: 450;
    text-align: start;
  }

  .nav-row:hover {
    background: var(--row-hover);
    color: var(--color-text);
  }

  .nav-row[aria-current="page"] {
    background: var(--row-active);
    color: var(--color-text);
    font-weight: 550;
  }

  .nav-row > span {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .nav-row > small {
    min-width: 16px;
    color: var(--color-faint);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    text-align: end;
  }

  .nav-row:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .navigation-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-block: 22px 6px;
    padding-inline-start: 10px;
    color: var(--color-faint);
    font-size: 11px;
    font-weight: 550;
  }

  .navigation-secondary {
    display: grid;
    flex: none;
    gap: 3px;
    padding-block-start: 10px;
    border-block-start: 1px solid var(--color-border);
  }

  .list-row {
    display: flex;
    align-items: center;
    min-width: 0;
    position: relative;
  }

  /* The count and the list's actions share one place: at rest the row says how
     much is in it, under the pointer it offers what can be done to it. */
  .list-row:hover > .nav-row > small,
  .list-row:focus-within > .nav-row > small {
    visibility: hidden;
  }

  .list-row :global(.list-menu) {
    position: absolute;
    inset-inline-end: 6px;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-inset);
    background: transparent;
    color: var(--color-muted);
    opacity: 0;
  }

  .list-row :global(.list-menu:hover),
  .list-row :global(.list-menu[data-state="open"]) {
    background: var(--row-pressed);
    color: var(--color-text);
  }

  .list-row:hover :global(.list-menu),
  .list-row:focus-within :global(.list-menu),
  .list-row :global(.list-menu[data-state="open"]) {
    opacity: 1;
  }

  .list-editor {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-inline-start: 10px;
    color: var(--color-muted);
  }

  .list-editor > input {
    flex: 1;
    min-width: 0;
    width: 100%;
    padding: 7px 4px;
    border: 0;
    border-radius: 4px;
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    font-size: 13px;
  }

  .create-list {
    color: var(--color-faint);
  }

  .list-editor > input:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }
</style>
