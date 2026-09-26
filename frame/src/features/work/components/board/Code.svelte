<script lang="ts">
  import LazyView from "$shared/ui/LazyView";
  import { codeLines, loadCodeBlock } from "$shared/ui/data/Code";
  import { CODE_LINES } from "../../lib/board/size";
  import type { BoardActions } from "../../lib/canvas-context";
  import type { CodeBlock as Code } from "../../lib/board/types";
  import * as m from "$shared/i18n/messages";
  let {
    block,
    actions,
    open = false,
  }: { block: Code; actions?: BoardActions; open?: boolean } = $props();
  const count = $derived(codeLines(block.text).length);
  const more = $derived(count > CODE_LINES || (block.notes.length > 0 && !open));
</script>

<div class="code">
  <LazyView
    loader={loadCodeBlock}
    loadingLabel={m.surface_loading()}
    failureLabel={m.work_artifact_unavailable()}
    retryLabel={m.surface_retry()}
    >{#snippet children(CodeBlock)}<CodeBlock
        language={block.language}
        text={block.text}
        label={block.title ?? ""}
        notes={block.notes}
        variant={open ? "lift" : "card"}
        {...open ? {} : { limit: CODE_LINES }}
      />{/snippet}</LazyView
  >
</div>
{#if more || open}<footer>
    <button type="button" class="more nodrag nopan" onclick={() => actions?.toggle(block.id)}
      >{open
        ? m.work_board_fewer()
        : count > CODE_LINES
          ? m.work_board_all_lines({ count })
          : block.notes.length === 1
            ? m.work_code_note_one()
            : m.work_code_note_count({ count: block.notes.length })}</button
    >
  </footer>{/if}

<style>
  .code {
    overflow: hidden;
    padding: 10px 0;
    border-radius: var(--radius-row);
    background: var(--color-fill);
  }

  footer {
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

  .more:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>
