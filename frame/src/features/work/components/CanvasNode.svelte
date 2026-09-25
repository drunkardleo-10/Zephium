<script lang="ts">
  import { Handle, Position, type NodeProps } from "@xyflow/svelte";
  import type { WorkNode } from "../lib/canvas-model";
  import { getContext } from "svelte";
  import { canvasInspection } from "../lib/canvas-context";
  import * as m from "$shared/i18n/messages";
  const inspect = getContext<(id: string) => void>(canvasInspection);
  let { id, data, selected }: NodeProps<WorkNode> = $props();
</script>

<Handle
  type="target"
  position={Position.Left}
  isConnectable={false}
  tabindex={-1}
  aria-hidden="true"
/>
<article class:selected>
  <header class="work-drag-handle"><span>{data.kind}</span><strong>{data.title}</strong></header>
  <p>{data.detail}</p>
  <footer>
    <span>{data.status}</span><button
      type="button"
      class="nodrag nopan"
      onclick={(event) => {
        event.stopPropagation();
        inspect(id);
      }}
      aria-label={m.work_inspect_item({ title: data.title })}>{m.work_inspect()}</button
    >
  </footer>
</article>
<Handle
  type="source"
  position={Position.Right}
  isConnectable={false}
  tabindex={-1}
  aria-hidden="true"
/>

<style>
  article {
    height: 100%;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-card);
    background: var(--color-surface);
    color: var(--color-text);
    overflow: hidden;
  }

  article.selected {
    border-color: var(--color-accent);
  }

  header {
    display: grid;
    gap: 6px;
    padding: 14px 16px 8px;
    cursor: grab;
  }

  header span {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  strong {
    font-weight: 550;
    line-height: 1.4;
  }

  p {
    margin: 0;
    padding: 0 16px;
    color: var(--color-muted);
    line-height: 1.5;
    overflow: hidden;
    flex: 1;
  }

  footer {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 16px;
    color: var(--color-label-secondary);
    font-size: var(--text-caption);
  }

  button {
    font: inherit;
    background: var(--color-fill);
    color: var(--color-text);
    border: 0;
    border-radius: var(--radius-row);
    padding: 4px 8px;
    cursor: pointer;
  }

  button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>
