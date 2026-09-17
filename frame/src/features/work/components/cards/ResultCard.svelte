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

<CardFrame kind={item.kind} title={item.title} {icon} {selected}>
  {#if item.artifact}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      class="artifact-body nodrag nopan nowheel"
      role="region"
      aria-label={item.title}
      tabindex="0"
    >
      <Artifact
        artifact={item.artifact}
        embedded
        compact
        onevidence={evidence?.open ? (reference) => evidence.open?.(id, reference) : undefined}
        onlink={openLink}
      />
    </div>
  {:else}<p class="summary">{item.detail || item.status}</p>{/if}
  {#snippet footer()}<span>{item.status}</span>{#if item.actionLabel}<button
        type="button"
        class="link nodrag nopan"
        onclick={(event) => {
          event.stopPropagation();
          onaction();
        }}>{item.actionLabel}</button
      >{/if}{/snippet}
</CardFrame>

<style>
  .artifact-body {
    block-size: 100%;
    overflow: auto;
    font-size: var(--text-label);
  }

  .artifact-body:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
    border-radius: var(--radius-sm);
  }

  .summary {
    margin: 0;
    color: var(--color-muted);
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
  }

  .link:hover {
    background: var(--color-fill-hover);
  }
</style>
