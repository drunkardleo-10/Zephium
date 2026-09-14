<script lang="ts">
  import { Handle, Position, NodeResizer, type NodeProps } from "@xyflow/svelte";
  import type { WorkNode } from "../lib/canvas-model";
  import { getContext, tick } from "svelte";
  import {
    canvasInspection,
    canvasResize,
    canvasAction,
    canvasEvidence,
    canvasFocusResult,
  } from "../lib/canvas-context";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import Artifact from "$shared/ui/data/Artifact";
  import Icon from "$shared/ui/Icon";
  import { Task01Icon, Note01Icon } from "@hugeicons/core-free-icons";
  import FavIcon from "$shared/ui/FavIcon";
  import * as m from "$shared/i18n/messages";
  let artifactBody = $state<HTMLDivElement>();
  const focusResult = getContext<(id: string) => void>(canvasFocusResult);
  const inspect = getContext<(id: string) => void>(canvasInspection);
  const evidence = getContext<{ open?: (id: string, reference: EvidenceReference) => void }>(
    canvasEvidence,
  );
  const action = getContext<(id: string) => void>(canvasAction);
  const resize = getContext<(active: boolean) => void>(canvasResize);
  let { id, data, selected }: NodeProps<WorkNode> = $props();
</script>

<Handle
  type="target"
  position={Position.Left}
  isConnectable={false}
  tabindex={-1}
  aria-hidden="true"
/>
<NodeResizer
  onResizeStart={() => resize(true)}
  onResizeEnd={() => resize(false)}
  isVisible={selected}
  minWidth={120}
  minHeight={80}
  maxWidth={4096}
  maxHeight={4096}
/>
<article class:selected class:responsibility={!!data.responsibility} class:result={!!data.artifact}>
  <header class="work-drag-handle">
    <span>{data.kind}</span><strong
      >{#if data.responsibility}<Icon
          icon={Task01Icon}
          size={20}
        />{/if}{#if data.favicon !== undefined}<FavIcon
          favicon={data.favicon}
        />{/if}{data.title}</strong
    >
  </header>
  {#if data.responsibility}<div class="expected">
      {#if data.responsibility.outputs.length}<span>{m.work_output_description()}</span>
        <ul>
          {#each data.responsibility.outputs.slice(0, 3) as output, index (index)}<li>
              <Icon icon={Note01Icon} /><span>{output}</span>
            </li>{/each}
        </ul>
        {#if data.responsibility.outputs.length > 3}<span
            >+{data.responsibility.outputs.length - 3}</span
          >{/if}
      {/if}
    </div>
  {:else if data.artifact}<!-- Keyboard focus enables reading the full scrollable artifact. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      bind:this={artifactBody}
      class="artifact-body nodrag nopan nowheel"
      role="region"
      aria-label={data.title}
      tabindex="0"
    >
      <Artifact
        artifact={data.artifact}
        embedded
        onevidence={evidence?.open ? (reference) => evidence.open?.(id, reference) : undefined}
      />
    </div>{:else}<p>{data.detail}</p>{/if}
  <footer>
    {#if data.artifact}<button
        type="button"
        class="nodrag nopan"
        onclick={async (event) => {
          event.stopPropagation();
          focusResult(id);
          await tick();
          artifactBody?.focus({ preventScroll: true });
        }}>{m.work_env_focus_result()}</button
      >{/if}
    <span>{data.status}</span>{#if data.actionLabel}<button
        type="button"
        class="nodrag nopan"
        onclick={(event) => {
          event.stopPropagation();
          action(id);
        }}>{data.actionLabel}</button
      >{/if}<button
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
    border-radius: var(--radius-lg);
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
    display: flex;
    align-items: center;
    gap: 8px;
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

  .result strong {
    max-block-size: 2.8em;
    overflow: hidden;
  }

  .responsibility strong {
    font-size: var(--text-body);
    line-height: 1.35;
    max-block-size: 2.7em;
    overflow: hidden;
  }

  .responsibility strong :global(svg) {
    flex-shrink: 0;
    color: var(--color-accent);
  }

  .expected {
    flex: 1;
    min-block-size: 0;
    overflow: hidden;
    padding: 0 16px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .expected ul {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 6px 0 0;
    padding: 0;
    list-style: none;
  }

  .expected li {
    display: flex;
    align-items: center;
    gap: 4px;
    max-inline-size: 100%;
    padding: 4px 6px;
    border-radius: var(--radius-sm);
    background: var(--color-fill);
  }

  .expected li span {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .expected li :global(svg) {
    flex-shrink: 0;
  }

  .artifact-body {
    overflow: auto;
    flex: 1;
    min-block-size: 0;
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
    border-radius: var(--radius-sm);
    padding: 4px 8px;
    cursor: pointer;
  }

  button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>
