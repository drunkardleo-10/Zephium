<script lang="ts">
  import { untrack } from "svelte";
  import type { NoteSession } from "$domain/notes";
  import Icon from "$shared/ui/Icon";
  import { Delete02Icon, Note01Icon, Search01Icon } from "@hugeicons/core-free-icons";
  import PencilEdit02Icon from "@hugeicons/core-free-icons/PencilEdit02Icon";
  import * as m from "$shared/i18n/messages";
  import NoteList from "./NoteList.svelte";
  import NoteView from "./NoteView.svelte";
  import NoteNotice from "./NoteNotice.svelte";
  import { enter } from "../lib/enter";

  let { session }: { session: NoteSession } = $props();

  let open = $derived(session.note !== null);
  let direction = $state<"forward" | "back" | null>(null);
  let was = untrack(() => open);
  $effect.pre(() => {
    if (open !== was) direction = open ? "forward" : "back";
    was = open;
  });

  let scroll = $state<HTMLDivElement>();

  function edit() {
    scroll?.querySelector<HTMLElement>(".note-document")?.focus();
  }
</script>

<div class="notes" data-view={open ? "note" : "list"} data-note-bounds>
  {#if session.note}
    {#key session.note.version}
      <div class="note-scroll" bind:this={scroll} use:enter={direction}>
        <NoteView {session} density="panel" autofocus />
      </div>
    {/key}
  {:else}
    <div class="notes-list" use:enter={direction}>
      {#if !session.trash}<button
          type="button"
          class="new-note"
          onclick={() => void session.create()}
          ><Icon icon={PencilEdit02Icon} size={16} /><span>{m.note_new()}</span></button
        >{/if}
      {#if session.error && !session.items.length}
        <div class="notes-empty">
          <span class="empty-icon"><Icon icon={Note01Icon} size={22} /></span>
          <h3>{m.note_unavailable()}</h3>
          <p>{m.note_unavailable_body()}</p>
          <button type="button" class="empty-action" onclick={() => void session.reload()}
            >{m.note_try_again()}</button
          >
        </div>
      {:else if session.loaded && !session.items.length}
        <div class="notes-empty">
          {#if session.query.trim()}
            <span class="empty-icon"><Icon icon={Search01Icon} size={22} /></span>
            <h3>{m.note_empty_search()}</h3>
            <p>{m.note_empty_search_body()}</p>
          {:else if session.trash}
            <span class="empty-icon"><Icon icon={Delete02Icon} size={22} /></span>
            <h3>{m.note_trash_empty()}</h3>
            <p>{m.note_trash_hint()}</p>
          {:else}
            <span class="empty-icon"><Icon icon={Note01Icon} size={22} /></span>
            <h3>{m.note_empty_title()}</h3>
            <p>{m.note_empty_body()}</p>
          {/if}
        </div>
      {:else}
        <NoteList {session} density="panel" onopen={(id) => void session.open(id)} onedit={edit} />
      {/if}
    </div>
  {/if}
  <NoteNotice {session} />
</div>

<style>
  /* Sized by the host, never by a long title or preview inside it: without
     containment the rows' unshrunk text widens the whole panel. */
  .notes {
    container-type: inline-size;
    position: relative;
    display: flex;
    flex: 1 1 0;
    flex-direction: column;
    width: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .notes-list {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .note-scroll {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    padding: 6px 20px 0 22px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .new-note {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: none;
    height: 36px;
    margin: 0 8px 4px;
    padding-inline: 10px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: 14px;
    text-align: start;
    cursor: default;
    outline: none;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .new-note:hover {
    background: var(--row-hover);
    color: var(--color-text);
  }

  .new-note:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .notes-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 56px 28px;
    text-align: center;
    animation: empty-in var(--motion-slow) var(--ease-out);
  }

  @keyframes empty-in {
    from {
      opacity: 0;
    }
  }

  .empty-icon {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    margin-block-end: 8px;
    border-radius: var(--radius-control);
    background: var(--color-fill);
    color: var(--color-muted);
  }

  .notes-empty h3 {
    margin: 0;
    color: var(--color-text);
    font-size: 14px;
    font-weight: 600;
  }

  .notes-empty p {
    max-width: 260px;
    text-wrap: balance;
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-body);
    line-height: 19px;
  }

  .empty-action {
    height: 28px;
    margin-block-start: 10px;
    padding-inline: 12px;
    border: 0;
    border-radius: var(--radius-inset);
    background: var(--color-control);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    cursor: default;
  }

  .empty-action:hover {
    background: var(--color-control-hover);
  }
</style>
