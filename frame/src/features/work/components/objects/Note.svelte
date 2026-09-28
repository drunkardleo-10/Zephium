<script lang="ts">
  import { tick } from "svelte";
  import * as m from "$shared/i18n/messages";
  import type { Detail, NoteView, ObjectActions } from "../../lib/board/types";
  import Inline from "./Inline.svelte";
  import { noteBlocks } from "./markdown";
  /**
   * Words written on the canvas itself: no card. Click to write, and the text
   * grows as it is written; it is Markdown in the notes store underneath.
   */
  let {
    object,
    detail,
    actions = {},
  }: { object: NoteView; detail: Detail; actions?: ObjectActions } = $props();
  let editing = $state(false);
  let draft = $state("");
  let field = $state<HTMLTextAreaElement>();
  const parts = $derived(noteBlocks(object.markdown));
  async function begin() {
    if (!actions.write || detail !== "full") return;
    draft = object.markdown;
    editing = true;
    await tick();
    field?.focus();
    grow();
  }
  function end() {
    editing = false;
    if (draft !== object.markdown) actions.write?.(object.id, draft);
  }
  function grow() {
    if (!field) return;
    field.style.blockSize = "auto";
    field.style.blockSize = `${field.scrollHeight}px`;
  }
</script>

<div class="note {detail}">
  {#if editing}
    <textarea
      bind:this={field}
      bind:value={draft}
      class="nodrag nopan nowheel"
      aria-label={m.work_note_text()}
      oninput={grow}
      onblur={end}
      onkeydown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          field?.blur();
        }
      }}></textarea>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      class="text"
      role={actions.write ? "button" : undefined}
      tabindex={actions.write ? 0 : undefined}
      onclick={begin}
      onkeydown={(event) => {
        if (event.key === "Enter") void begin();
      }}
    >
      {#each detail === "full" ? parts : parts.slice(0, 1) as part, index (index)}
        {#if part.kind === "heading"}<h3><Inline text={part.text} /></h3>
        {:else if part.kind === "list"}<ul>
            {#each part.items as item, at (at)}<li><Inline text={item} /></li>{/each}
          </ul>
        {:else}<p>
            {#each part.lines as line, at (at)}{#if at}<br />{/if}<Inline text={line} />{/each}
          </p>{/if}
      {:else}<p class="empty">{m.work_note_empty()}</p>{/each}
    </div>
  {/if}
</div>

<style>
  .note {
    inline-size: 100%;
    max-inline-size: 60ch;
    color: var(--color-text);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-radius: var(--radius-inset);
    outline: none;
    cursor: text;
  }

  .text:focus-visible {
    box-shadow: 0 0 0 2px var(--color-ring);
  }

  h3 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 650;
    line-height: 1.2;
    letter-spacing: -0.015em;
  }

  p,
  ul {
    margin: 0;
    color: var(--color-label-secondary);
    font-size: var(--text-reading);
    line-height: 1.5;
  }

  ul {
    padding-inline-start: 20px;
    list-style: disc;
  }

  .empty {
    color: var(--color-faint);
  }

  textarea {
    box-sizing: border-box;
    inline-size: 100%;
    min-block-size: 3em;
    padding: 0;
    overflow: hidden;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-reading);
    line-height: 1.5;
    resize: none;
    outline: none;
    caret-color: var(--color-text);
  }

  .overview h3,
  .overview p {
    font-size: var(--text-overview-title);
  }

  .tile h3,
  .tile p {
    font-size: var(--text-tile-title);
  }
</style>
