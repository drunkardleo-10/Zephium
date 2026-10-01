<script lang="ts">
  import DocumentView from "$shared/ui/data/Artifact/DocumentView.svelte";
  import MakeTasks from "./MakeTasks.svelte";
  import type { BoardActions } from "../../lib/canvas-context";
  import type { DocumentBlock } from "../../lib/board/types";
  import * as m from "$shared/i18n/messages";
  let {
    block,
    actions,
    open = false,
  }: { block: DocumentBlock; actions?: BoardActions; open?: boolean } = $props();
  const content = $derived(block.content);
  /** Unformatted paragraphs as they were written: headings, numbered and plain lists, prose. */
  const lines = $derived(
    content.paragraphs.flatMap((paragraph) =>
      paragraph
        .split("\n")
        .map((line) => line.trim())
        .filter(Boolean)
        .map((line) => {
          const heading = /^(#{1,6})\s+(.*)$/u.exec(line);
          if (heading)
            return { kind: "heading" as const, text: heading[2]!, level: heading[1]!.length };
          const item = /^(?:[-*•]|\d+[.)])\s+(.*)$/u.exec(line);
          if (item) return { kind: "item" as const, text: item[1]!, level: 0 };
          return { kind: "text" as const, text: line, level: 0 };
        }),
    ),
  );
  const note = $derived(actions?.note(block.id));
</script>

<div class="document nowheel" class:open>
  {#if !open}<DocumentView card document={content.formatted} paragraphs={content.paragraphs} />
  {:else if content.formatted}<DocumentView document={content.formatted} onlink={actions?.page} />
  {:else}
    {#each lines as line, index (index)}
      {#if line.kind === "heading"}{#if line.level > 1}<h4>{line.text}</h4>{/if}
      {:else if line.kind === "item"}<p class="item">{line.text}</p>
      {:else}<p>{line.text}</p>{/if}
    {/each}
  {/if}
</div>
<footer>
  <button type="button" class="more nodrag nopan" onclick={() => actions?.toggle(block.id)}
    >{open ? m.work_board_fewer() : m.work_board_read_all()}</button
  >
  <MakeTasks id={block.id} />
  {#if note}<button
      type="button"
      class="more nodrag nopan"
      disabled={note.disabled}
      onclick={() => note.onclick?.()}>{note.label}</button
    >{/if}
</footer>

<style>
  .document {
    max-inline-size: 72ch;
    color: var(--color-text);
    font-size: var(--text-body);
    line-height: 20px;
  }

  .open {
    max-block-size: 720px;
    overflow-y: auto;
  }

  p {
    margin: 0 0 10px;
  }

  .item {
    position: relative;
    padding-inline-start: 16px;
  }

  .item::before {
    content: "";
    position: absolute;
    inset-block-start: 8px;
    inset-inline-start: 4px;
    inline-size: 4px;
    block-size: 4px;
    border-radius: var(--radius-capsule);
    background: var(--color-faint);
  }

  h4 {
    margin: 16px 0 6px;
    font-size: var(--text-body);
    font-weight: 600;
  }

  footer {
    display: flex;
    gap: 6px;
    margin-block-start: 12px;
  }

  .more {
    block-size: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .more:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>
