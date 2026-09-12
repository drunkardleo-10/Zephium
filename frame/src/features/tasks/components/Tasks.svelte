<script lang="ts">
  import { untrack } from "svelte";
  import { resourceSession, type ResourceSession } from "$domain/resources";
  import ResourcePanel from "$shared/ui/data/ResourcePanel";
  import SaveStatus from "$shared/ui/data/SaveStatus";
  import Button from "$shared/ui/Button";
  import TaskCard from "./TaskCard.svelte";
  import * as m from "$shared/i18n/messages";
  let {
    profile,
    host = "work",
    onclose,
    ondrag,
    onback,
  }: {
    profile: string;
    host?: string;
    onclose?: () => void;
    ondrag?: () => void;
    onback?: () => void;
  } = $props();
  let session = $state.raw<ResourceSession | null>(
    untrack(() => resourceSession(profile, "task", host)),
  );
  $effect(() => {
    const owner = profile;
    const scope = host;
    const current = untrack(() => resourceSession(owner, "task", scope));
    session = current;
    void current?.start();
    return () => current?.stopObserving();
  });
  let titleInput = $state<HTMLInputElement>();
  async function create() {
    const current = session;
    await current?.create(m.task_untitled());
    if (current === session && current?.record && current.saveState === "saved") {
      titleInput?.focus();
      titleInput?.select();
    }
  }
  async function close() {
    const current = session;
    if (current && (await current.flush()) && current === session) onclose?.();
  }
  async function exit() {
    const current = session;
    if (current && (await current.flush()) && current === session) onback?.();
  }
  async function trashFilter() {
    const current = session;
    if (current && (await current.flush()) && current === session) {
      await current.back();
      if (current !== session) return;
      current.trash = !current.trash;
      void current.reload();
    }
  }
</script>

{#if session}<ResourcePanel
    title={m.tool_tasks()}
    rows={session.items}
    query={session.query}
    onquery={(value) => session?.search(value)}
    loading={session.loading}
    error={session.error}
    editing={!!session.draft}
    oncreate={create}
    onclose={onclose ? close : undefined}
    onback={() => {
      void session?.back();
    }}
    {ondrag}
    onexit={onback ? exit : undefined}
    hasMore={!!session.next}
    onmore={() => {
      void session?.reload(true);
    }}
    trash={session.trash}
    ontrash={trashFilter}
  >
    {#snippet status()}{#if session && (session.draft || session.pending)}<SaveStatus
          state={session.saveState}
          onretry={() => {
            void session?.retry();
          }}
          ondiscard={() => {
            void session?.discardDraft();
          }}
          onkeep={() => {
            void session?.keepDraft();
          }}
        />{/if}{/snippet}
    {#snippet filters()}<div class="task-filters">
        {#each ["all", "open", "completed"] as filter (filter)}<Button
            variant="ghost"
            size="compact"
            aria-pressed={session?.filter === filter}
            onclick={() => {
              if (session) {
                session.filter = filter as "all" | "open" | "completed";
                void session.reload();
              }
            }}
            >{filter === "all"
              ? m.tool_all()
              : filter === "open"
                ? m.task_open()
                : m.tool_completed()}</Button
          >{/each}
      </div>{/snippet}
    {#snippet row(task)}<TaskCard
        title={task.title}
        completed={task.completed ?? false}
        dueDate={task.due_date}
        pinned={task.pinned}
        selected={session?.record?.id === task.id}
        disabled={session?.navigating ||
          session?.saveState === "saving" ||
          session?.saveState === "unknown" ||
          session?.trash}
        onopen={() => {
          void session?.open(task.id);
        }}
        ontoggle={(value) => {
          void session?.setTaskCompleted(task, value);
        }}
      />{/snippet}
    {#snippet editor()}{#if session?.draft?.content.kind === "task"}{@const current =
          session}{@const task = session.draft.content}
        <div class="task-editor">
          <label class="task-title"
            ><span>{m.resource_title()}</span><input
              bind:this={titleInput}
              value={current.draft!.title}
              maxlength="256"
              required
              disabled={!current.canEdit}
              oninput={(event) => current.edit({ title: event.currentTarget.value })}
            /></label
          >
          <button
            type="button"
            class="completion"
            role="checkbox"
            aria-checked={current.record?.draft.content.kind === "task" &&
              current.record.draft.content.completed}
            disabled={current.navigating ||
              current.record?.trashed ||
              current.saveState === "unknown" ||
              current.saveState === "conflict" ||
              task.completed !==
                (current.record?.draft.content.kind === "task" &&
                  current.record.draft.content.completed)}
            onclick={() => {
              current.edit({ content: { ...task, completed: !task.completed } });
              void current.flush();
            }}
            ><span aria-hidden="true"
              >{current.record?.draft.content.kind === "task" &&
              current.record.draft.content.completed
                ? "✓"
                : "○"}</span
            >{m.task_complete()}</button
          >
          <label
            >{m.task_description()}<textarea
              value={task.description}
              rows="4"
              maxlength="4096"
              disabled={!current.canEdit}
              oninput={(event) =>
                current.edit({ content: { ...task, description: event.currentTarget.value } })}
            ></textarea></label
          >
          <label
            >{m.task_due()}<input
              type="date"
              value={task.due_date ?? ""}
              disabled={!current.canEdit}
              onchange={(event) =>
                current.edit({ content: { ...task, due_date: event.currentTarget.value || null } })}
            /></label
          >
          <div class="task-actions">
            <Button
              size="compact"
              disabled={!current.canEdit}
              aria-pressed={current.draft!.pinned}
              onclick={() => current.edit({ pinned: !current.draft!.pinned })}
              >{m.resource_pin()}</Button
            >{#if current.record}<Button
                size="compact"
                disabled={current.navigating}
                onclick={() => {
                  void current.setTrashed(!current.record!.trashed);
                }}>{current.record.trashed ? m.resource_restore() : m.resource_trash()}</Button
              >{/if}
          </div>
        </div>
      {/if}{/snippet}
  </ResourcePanel>{:else}<p role="alert">{m.resource_draft_capacity()}</p>{/if}

<style>
  .completion {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px;
    text-align: start;
    font: inherit;
    background: var(--color-fill);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  .completion:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .completion:disabled {
    opacity: 0.5;
  }

  .task-editor {
    display: grid;
    gap: 20px;
  }

  label {
    display: grid;
    gap: 8px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  input,
  textarea {
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    padding: 12px;
    font: inherit;
    font-size: 14px;
    line-height: 1.6;
    color: var(--color-text);
    background: var(--color-field);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-field);
  }

  textarea {
    resize: vertical;
  }

  input:focus-visible,
  textarea:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .task-title input {
    padding: 8px 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    font-size: var(--text-title);
    font-weight: 600;
    line-height: 1.3;
  }

  .task-title input:focus-visible {
    outline: none;
    box-shadow: 0 2px var(--color-ring);
  }

  .task-actions,
  .task-filters {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .task-filters {
    margin-block-start: 12px;
  }
</style>
