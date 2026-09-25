<script lang="ts">
  import { getContext } from "svelte";
  import CardFrame from "./CardFrame.svelte";
  import Artifact from "$shared/ui/data/Artifact";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import {
    ChartColumnIcon,
    CheckListIcon,
    Doc01Icon,
    GitCompareIcon,
    GlobalIcon,
    Link04Icon,
    Table01Icon,
  } from "../../lib/icons";
  import { canvasEvidence, canvasOpenLink } from "../../lib/canvas-context";
  import type { CanvasItem } from "../../lib/canvas-model";
  let {
    id,
    item,
    selected,
    onaction,
  }: { id: string; item: CanvasItem; selected: boolean; onaction: () => void } = $props();
  const openLink = getContext<((href: string) => void) | undefined>(canvasOpenLink);
  const evidence = getContext<{ open?: (id: string, reference: EvidenceReference) => void }>(
    canvasEvidence,
  );
  const icon = $derived.by(() => {
    switch (item.artifact?.content.kind) {
      case "comparison":
        return GitCompareIcon;
      case "table":
        return Table01Icon;
      case "chart":
        return ChartColumnIcon;
      case "checklist":
        return CheckListIcon;
      case "sources":
        return Link04Icon;
      case "browser":
        return GlobalIcon;
      default:
        return Doc01Icon;
    }
  });
</script>

<!-- The hero of a stage: the answer itself, not the sources it cites. -->
<!-- State belongs to the agent line: the card says only the answer, and its one action. -->
<CardFrame title={item.title} {icon} {selected} footer={item.actionLabel ? action : undefined}>
  {#if item.artifact}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      class="artifact-body nodrag nopan"
      class:end={!item.actionLabel}
      role="region"
      aria-label={item.title}
      tabindex="0"
    >
      <Artifact
        artifact={item.artifact}
        embedded
        card
        onevidence={evidence?.open ? (reference) => evidence.open?.(id, reference) : undefined}
        onlink={openLink}
      />
    </div>
  {:else}<p class="summary">{item.detail || item.status}</p>{/if}
</CardFrame>
{#snippet action()}<span></span><button
    type="button"
    class="link nodrag nopan"
    onclick={(event) => {
      event.stopPropagation();
      onaction();
    }}>{item.actionLabel}</button
  >{/snippet}

<style>
  .artifact-body {
    box-sizing: border-box;
    block-size: 100%;
    overflow: hidden;
    font-size: var(--text-label);
  }

  .artifact-body.end {
    padding-block-end: 14px;
  }

  .artifact-body:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
    border-radius: var(--radius-row);
  }

  .summary {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    margin: 0;
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
  }

  .link {
    border: 0;
    padding: 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-smooth);
  }

  .link:hover {
    background: var(--color-fill-hover);
  }
</style>
