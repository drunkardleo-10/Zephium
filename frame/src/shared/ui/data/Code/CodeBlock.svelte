<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { noteOwners, type CodeNoteView } from "./code";
  import { tokenize } from "./tokenize";
  let {
    language,
    text,
    label,
    notes = [],
    limit,
    variant = "card",
    start = 1,
  }: {
    language: string;
    text: string;
    label: string;
    notes?: readonly CodeNoteView[];
    limit?: number;
    variant?: "card" | "lift";
    /** The first line's number in its file. */
    start?: number;
  } = $props();
  const rows = $derived(tokenize(language, text, limit));
  const owners = $derived(noteOwners(notes, rows.length));
  let hovered = $state<number>();
  let lit = $state<number>();
  let body = $state<HTMLElement>();
  const span = (note: CodeNoteView) =>
    note.from === note.to
      ? m.work_code_line({ line: note.from })
      : m.work_code_lines({ from: note.from, to: note.to });
  function light(index: number) {
    lit = index;
    body
      ?.querySelector(`[data-line="${Math.max(1, notes[index]?.from ?? 1)}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }
</script>

<div class="code {variant}" style:--digits={String(rows.length + start - 1).length}>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <pre
    bind:this={body}
    aria-label={label}
    tabindex={variant === "lift" ? 0 : undefined}
    onpointerleave={() => (hovered = undefined)}><code
      >{#each rows as row, index (index)}{@const owner = owners[index]}<span
          class="row"
          data-line={index + 1}
          class:noted={owner !== undefined}
          class:lit={owner !== undefined && owner === lit}
          role="presentation"
          onpointerenter={() => (hovered = owner)}
          ><span class="n" aria-hidden="true">{index + start}</span
          >{#each row as token, at (at)}{#if token.kind === "plain" || token.kind === "punctuation"}{token.text}{:else}<span
                class={token.kind}>{token.text}</span
              >{/if}{/each}{#if variant === "card" && owner !== undefined && owner === hovered && index + 1 === Math.max(1, notes[owner]!.from)}<span
              class="note"
              role="note">{notes[owner]!.text}</span
            >{/if}</span
        >{/each}</code
    ></pre>
  {#if variant === "lift" && notes.length}<ol class="rail" aria-label={m.work_code_notes()}>
      {#each notes as note, index (index)}<li>
          <button type="button" aria-pressed={lit === index} onclick={() => light(index)}
            ><span class="range">{span(note)}</span><span>{note.text}</span></button
          >
        </li>{/each}
    </ol>{/if}
</div>

<style>
  .code {
    --line: 1.6em;

    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-label);
    min-width: 0;
  }

  .lift {
    display: grid;
    font-size: var(--text-body);
    gap: 16px;
    grid-template-columns: minmax(0, 1fr);
  }

  .lift:has(.rail) {
    grid-template-columns: minmax(0, 1fr) minmax(160px, 240px);
  }

  pre {
    font: inherit;
    margin: 0;
    overflow: clip visible;
  }

  .lift pre {
    overflow-x: auto;
  }

  code {
    display: block;
    font: inherit;
    min-width: max-content;
  }

  .card code {
    min-width: 0;
  }

  .row {
    display: block;
    line-height: var(--line);
    min-height: var(--line);
    padding-inline-end: 8px;
    position: relative;
    white-space: pre;
  }

  .n {
    background: var(--color-surface);
    box-shadow: inset 2px 0 transparent;
    color: var(--color-muted);
    display: inline-block;
    font-variant-numeric: tabular-nums;
    left: 0;
    margin-inline-end: 12px;
    padding-inline: 6px 0;
    position: sticky;
    text-align: end;
    user-select: none;
    width: calc(var(--digits) * 1ch + 6px);
  }

  .card .n {
    background: none;
  }

  .noted .n {
    box-shadow: inset 2px 0 var(--color-lit);
  }

  .lit {
    background: var(--color-lit-soft);
  }

  .keyword {
    color: var(--color-text);
    font-weight: 600;
  }

  .string {
    color: var(--color-label-secondary);
  }

  .comment {
    color: var(--color-muted);
    font-style: italic;
  }

  .number {
    color: var(--color-text);
  }

  .note {
    background: var(--color-float);
    border-radius: var(--radius-inset);
    box-shadow: var(--shadow-popover);
    color: var(--color-text);
    font-family: var(--font-sans);
    inset-block-start: calc(var(--line) + 2px);
    inset-inline-end: 8px;
    line-height: 1.4;
    max-width: 70%;
    padding: 6px 8px;
    pointer-events: none;
    position: absolute;
    white-space: normal;
    z-index: 1;
  }

  .rail {
    align-content: start;
    display: grid;
    gap: 4px;
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .rail button {
    background: none;
    border: 0;
    border-radius: var(--radius-row);
    color: var(--color-text);
    display: grid;
    font: inherit;
    font-family: var(--font-sans);
    gap: 2px;
    padding: 8px 10px;
    text-align: start;
    width: 100%;
  }

  .rail button:hover {
    background: var(--color-control-hover);
  }

  .rail button[aria-pressed="true"] {
    background: var(--color-lit-soft);
  }

  .range {
    color: var(--color-muted);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
  }
</style>
