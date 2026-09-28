<script lang="ts">
  import { onMount } from "svelte";
  import type { NoteSummary } from "$shared/ipc/bindings";
  import type { NoteSession } from "$domain/notes";
  import Icon from "$shared/ui/Icon";
  import SearchField from "$shared/ui/SearchField";
  import { Add01Icon, Note01Icon, Tick02Icon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";

  let {
    notes,
    placed,
    busy = false,
    oncreate,
    onpick,
  }: {
    notes: NoteSession;
    /** Notes already on this canvas: shown, not offered twice. */
    placed: readonly string[];
    busy?: boolean;
    oncreate: () => void;
    onpick: (id: string) => void;
  } = $props();

  let query = $state("");
  let items = $state.raw<NoteSummary[]>([]);
  let loaded = $state(false);
  let field = $state<HTMLInputElement>();
  let asked = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function look(text: string) {
    const request = ++asked;
    await notes.start();
    const found = await notes.find(text);
    if (request !== asked) return;
    items = found;
    loaded = true;
  }

  onMount(() => {
    void look("");
    field?.focus();
    return () => clearTimeout(timer);
  });
</script>

<!-- A note joins the canvas by choice: a new one, or one of the person's own. -->
<div class="note-panel">
  <button type="button" class="ui-menu-item create" disabled={busy} onclick={oncreate}>
    <span class="ui-menu-icon" aria-hidden="true"><Icon icon={Add01Icon} size={15} /></span>
    {m.work_note_new()}
  </button>
  <div class="ui-menu-separator" role="presentation"></div>
  <div class="search">
    <SearchField
      label={m.work_note_search()}
      placeholder={m.work_note_search()}
      bind:value={query}
      bind:ref={field}
      oninput={(text) => {
        clearTimeout(timer);
        timer = setTimeout(() => void look(text.trim()), 160);
      }}
      onsubmit={() => {
        const first = items.find((item) => !placed.includes(item.id));
        if (first) onpick(first.id);
      }}
    />
  </div>
  <ul class="list" aria-label={m.work_note_existing()}>
    {#each items as note (note.id)}
      {@const here = placed.includes(note.id)}
      <li>
        <button
          type="button"
          class="ui-menu-item note"
          disabled={busy || here}
          onclick={() => onpick(note.id)}
        >
          <span class="ui-menu-icon" aria-hidden="true"><Icon icon={Note01Icon} size={15} /></span>
          <span class="text">
            <span class="title">{note.title || m.note_untitled()}</span>
            {#if note.preview}<span class="preview">{note.preview}</span>{/if}
          </span>
          {#if here}<span class="mark" aria-label={m.work_note_on_canvas()}
              ><Icon icon={Tick02Icon} size={14} strokeWidth={2} /></span
            >{/if}
        </button>
      </li>
    {:else}{#if loaded}<li class="empty">
          {query.trim() ? m.work_note_none_match() : m.work_note_none()}
        </li>{/if}{/each}
  </ul>
</div>

<style>
  .note-panel {
    display: flex;
    flex-direction: column;
    max-block-size: min(420px, calc(100vh - 160px));
  }

  button.ui-menu-item {
    inline-size: 100%;
    border: 0;
    background: transparent;
    font: inherit;
    text-align: start;
  }

  button.ui-menu-item:disabled {
    opacity: 0.45;
  }

  button.ui-menu-item:focus-visible,
  button.ui-menu-item:hover:not(:disabled) {
    background: var(--row-active);
  }

  .search {
    padding: 2px 2px 6px;
  }

  .list {
    flex: 1;
    min-block-size: 0;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    list-style: none;
  }

  .note {
    align-items: flex-start;
  }

  .note .ui-menu-icon {
    margin-block-start: 1px;
  }

  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 1px;
    min-inline-size: 0;
  }

  .title,
  .preview {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .preview {
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
  }

  .mark {
    display: grid;
    flex: none;
    color: var(--color-muted);
    place-items: center;
  }

  .empty {
    padding: 10px var(--menu-item-inset);
    color: var(--color-faint);
  }
</style>
