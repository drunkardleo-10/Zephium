<script lang="ts">
  import { Handle, Position, NodeResizer, NodeToolbar, type NodeProps } from "@xyflow/svelte";
  import { getContext, tick } from "svelte";
  import Icon from "$shared/ui/Icon";
  import type { WorkItemNode } from "../lib/canvas-model";
  import { canvasResize, canvasAction, canvasOpen, canvasFocusResult } from "../lib/canvas-context";
  import { ArrowUpRight01Icon, MinusSignIcon, Target01Icon } from "../lib/icons";
  import TabCard from "./cards/TabCard.svelte";
  import NoteCard from "./cards/NoteCard.svelte";
  import ObjectiveCard from "./cards/ObjectiveCard.svelte";
  import ResponsibilityCard from "./cards/ResponsibilityCard.svelte";
  import ResultCard from "./cards/ResultCard.svelte";
  import SubjectCard from "./cards/SubjectCard.svelte";
  import FindingCard from "./cards/FindingCard.svelte";
  import AgentCard from "./cards/AgentCard.svelte";
  import * as m from "$shared/i18n/messages";
  const open = getContext<(id: string) => void>(canvasOpen);
  const action = getContext<(id: string, action?: string) => void>(canvasAction);
  const resize = getContext<(active: boolean) => void>(canvasResize);
  const focusResult = getContext<(id: string) => void>(canvasFocusResult);
  let { id, data, selected }: NodeProps<WorkItemNode> = $props();
  let root = $state<HTMLDivElement>();
  const type = $derived(data.type ?? (data.artifact ? "result" : "objective"));
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
  minWidth={160}
  minHeight={72}
  maxWidth={4096}
  maxHeight={4096}
  lineClass="work-resize-line"
  handleClass="work-resize-handle"
/>
<NodeToolbar isVisible={selected} position={Position.Top} offset={10}>
  <div class="bar" role="toolbar" aria-label={data.title}>
    <button type="button" onclick={() => open(id)}>
      <Icon icon={ArrowUpRight01Icon} size={14} />{m.work_env_open()}
    </button>
    {#if data.artifact}<button
        type="button"
        onclick={async () => {
          focusResult(id);
          await tick();
          root?.querySelector<HTMLElement>(".artifact-body")?.focus({ preventScroll: true });
        }}
      >
        <Icon icon={Target01Icon} size={14} />{m.work_env_focus_result()}
      </button>{/if}
    <button type="button" onclick={() => action(id, "inspect")}>{m.work_inspect()}</button>
    {#if type !== "responsibility" && type !== "agent" && !data.actionLabel}<span class="separator"
      ></span><button type="button" class="danger" onclick={() => action(id, "remove")}>
        <Icon icon={MinusSignIcon} size={14} />{m.work_env_remove()}
      </button>{/if}
  </div>
</NodeToolbar>
<div class="node-root" bind:this={root}>
  {#if type === "tab"}<TabCard item={data} {selected} />
  {:else if type === "subject"}<SubjectCard item={data} {selected} />
  {:else if type === "finding"}<FindingCard item={data} {selected} />
  {:else if type === "agent"}<AgentCard item={data} {selected} />
  {:else if type === "note"}<NoteCard item={data} {selected} />
  {:else if type === "responsibility"}<ResponsibilityCard item={data} {selected} />
  {:else if type === "result" || data.artifact}<ResultCard
      {id}
      item={data}
      {selected}
      onaction={() => action(id)}
    />
  {:else}<ObjectiveCard item={data} {selected} onaction={() => action(id)} />{/if}
</div>
<Handle
  type="source"
  position={Position.Right}
  isConnectable={false}
  tabindex={-1}
  aria-hidden="true"
/>

<style>
  .node-root {
    display: contents;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    border-radius: var(--radius-control);
    background: var(--color-menu);
    backdrop-filter: blur(12px) saturate(1.2);
    box-shadow: var(--shadow-popover);
  }

  .bar button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    block-size: 28px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
    transition: background-color var(--motion-instant) ease;
  }

  .bar button:hover {
    background: var(--color-control-hover);
  }

  .bar button.danger:hover {
    color: var(--color-danger);
  }

  .separator {
    inline-size: 1px;
    block-size: 18px;
    margin-inline: 4px;
    background: var(--color-border);
  }

  :global(.work-resize-line) {
    border-color: transparent !important;
  }

  :global(.work-resize-handle) {
    inline-size: 8px !important;
    block-size: 8px !important;
    border-radius: 50% !important;
    border: 1px solid var(--color-border-strong) !important;
    background: var(--color-surface) !important;
  }
</style>
