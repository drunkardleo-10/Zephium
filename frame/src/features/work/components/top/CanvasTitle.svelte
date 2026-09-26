<script lang="ts">
  import { tick } from "svelte";
  import * as m from "$shared/i18n/messages";

  let {
    title,
    disabled = false,
    status = null,
    onrename,
  }: {
    title: string;
    disabled?: boolean;
    /** What the project's run is doing, in a word or two, while it does it. */
    status?: string | null;
    onrename: (title: string) => void;
  } = $props();
  let editing = $state(false);
  let draft = $state("");
  let field = $state<HTMLInputElement>();

  async function edit() {
    if (disabled) return;
    draft = title;
    editing = true;
    await tick();
    field?.focus();
    field?.select();
  }
  function commit() {
    if (!editing) return;
    editing = false;
    const next = draft.trim();
    if (next && next !== title) onrename(next);
  }
</script>

<!-- The project's name where a document keeps its own: in place, and edited there. -->
<div class="canvas-title">
  {#if editing}
    <span class="edit"
      ><span class="sizer" aria-hidden="true">{draft || " "}</span><input
        bind:this={field}
        bind:value={draft}
        class="name"
        size="1"
        aria-label={m.work_project_title()}
        maxlength="128"
        onblur={commit}
        onkeydown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            commit();
          } else if (event.key === "Escape") {
            event.preventDefault();
            editing = false;
          }
        }}
      /></span
    >
  {:else}
    <button
      type="button"
      class="name"
      aria-label={m.work_project_rename_named({ title })}
      {disabled}
      onclick={() => void edit()}>{title}</button
    >
  {/if}
  {#if status}<span class="status" role="status"
      ><span class="live" aria-hidden="true"></span>{status}</span
    >{/if}
</div>

<style>
  .canvas-title {
    display: flex;
    align-items: center;
    gap: 10px;
    min-inline-size: 0;
    pointer-events: auto;
  }

  .name,
  .sizer {
    box-sizing: border-box;
    max-inline-size: min(420px, 50vw);
    block-size: 30px;
    padding: 0 8px;
    font: inherit;
    font-size: var(--text-body);
    font-weight: 600;
    letter-spacing: -0.005em;
    white-space: pre;
  }

  .name {
    overflow: hidden;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-text);
    text-align: start;
    text-overflow: ellipsis;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  button.name:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 0;
  }

  button.name:hover:not(:disabled) {
    background: var(--color-fill-hover);
  }

  /* The field is exactly as wide as the name in it: both share one cell, and
     the hidden copy of the words sets its width. */
  .edit {
    display: inline-grid;
    grid-template-columns: minmax(80px, auto);
    min-inline-size: 0;
  }

  .edit > * {
    grid-area: 1 / 1;
  }

  .sizer {
    visibility: hidden;
    overflow: hidden;
  }

  input.name {
    inline-size: 100%;
    min-inline-size: 0;
    background: var(--color-field);
    box-shadow: var(--shadow-field-focus);
    outline: none;
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    color: var(--color-muted);
    font-size: var(--text-label);
    white-space: nowrap;
  }

  .live {
    inline-size: 6px;
    block-size: 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-lit) 18%, transparent);
  }
</style>
