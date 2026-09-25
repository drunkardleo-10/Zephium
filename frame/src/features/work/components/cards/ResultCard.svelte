<script lang="ts">
  import { getContext } from "svelte";
  import CardFrame from "./CardFrame.svelte";
  import TableCard from "./TableCard.svelte";
  import Icon from "$shared/ui/Icon";
  import { SparklesIcon } from "../../lib/icons";
  import { TABLE_CARD, tableGrid } from "$shared/ui/data/Artifact/table";
  import Artifact, { documentDigest } from "$shared/ui/data/Artifact";
  import { cardKnowledge } from "$shared/ui/data/Artifact/artifact";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import { canvasEvidence, canvasOpen, canvasOpenLink } from "../../lib/canvas-context";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let {
    id,
    item,
    selected,
    onaction,
  }: { id: string; item: CanvasItem; selected: boolean; onaction: () => void } = $props();
  const open = getContext<((id: string) => void) | undefined>(canvasOpen);
  const openLink = getContext<((href: string) => void) | undefined>(canvasOpenLink);
  const evidence = getContext<{ open?: (id: string, reference: EvidenceReference) => void }>(
    canvasEvidence,
  );
  /** What the cover is, in one word. */
  const kind = $derived.by(() => {
    const content = item.artifact?.content;
    switch (content?.kind) {
      case "document":
        return documentDigest(content).steps.length
          ? m.work_card_kind_plan()
          : m.work_card_kind_document();
      case "checklist":
        return m.work_card_kind_checklist();
      case "chart":
        return m.work_env_component_chart();
      case "table":
        return m.work_env_component_table();
      case "comparison":
      case "matrix":
        return m.work_lift_comparison();
      case "findings":
        return m.work_card_findings();
      case "sources":
        return m.work_env_sources();
      case "browser":
        return m.work_env_page();
      case "diagram":
        return m.work_card_kind_diagram();
      default:
        return item.kind;
    }
  });
  const content = $derived(item.artifact?.content);
  const tabular = $derived(
    content?.kind === "table" || content?.kind === "comparison" ? content : undefined,
  );
  /** Rows past the six a table card shows. */
  const hidden = $derived(tabular ? tableGrid(tabular).rows.length - TABLE_CARD.rows : 0);
  // One caption per card: the card's own, never the chart's as well.
  const knowledge = $derived(!!item.artifact && cardKnowledge(item.artifact));
  // A press that moved was a drag; a control inside the cover keeps its own click.
  let pressed: { x: number; y: number } | null = null;
  function openCover(event: MouseEvent) {
    const from = pressed;
    pressed = null;
    if (from && Math.hypot(event.clientX - from.x, event.clientY - from.y) > 4) return;
    if ((event.target as Element).closest("button:not(:disabled), a, input, select")) return;
    open?.(id);
  }
</script>

<!-- A cover: what it is, its title, the whole summary and its sections; the lift has the rest. -->
<CardFrame
  {id}
  {kind}
  title={item.title}
  {selected}
  footer={item.actionLabel || knowledge || hidden > 0 ? action : undefined}
>
  {#if item.artifact}
    <div
      class="artifact-body"
      role="button"
      aria-label={item.title}
      tabindex="0"
      onpointerdown={(event) => (pressed = { x: event.clientX, y: event.clientY })}
      onclick={openCover}
      onkeydown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        open?.(id);
      }}
    >
      {#if tabular}<TableCard content={tabular} />{:else}<Artifact
          artifact={item.artifact}
          embedded
          card
          onevidence={evidence?.open ? (reference) => evidence.open?.(id, reference) : undefined}
          onlink={openLink}
        />{/if}
    </div>
  {:else}<p class="summary">{item.detail || item.status}</p>{/if}
</CardFrame>
{#snippet action()}<span class="notes">
    {#if knowledge}<span class="knowledge"
        ><Icon icon={SparklesIcon} size={11} />{m.work_knowledge_caption()}</span
      >{/if}
    {#if hidden > 0}<span class="rows"
        >{hidden === 1
          ? m.work_table_more_row_one()
          : m.work_table_more_rows({ count: hidden })}</span
      >{/if}
  </span>{#if item.actionLabel}<button
      type="button"
      class="link nodrag nopan"
      onclick={(event) => {
        event.stopPropagation();
        onaction();
      }}>{item.actionLabel}</button
    >{/if}{/snippet}

<style>
  .artifact-body {
    box-sizing: border-box;
    block-size: 100%;
    overflow: hidden;
    font-size: var(--text-label);
    cursor: default;
  }

  .artifact-body:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
    border-radius: var(--radius-inset);
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

  .notes {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
  }

  .knowledge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--color-muted);
  }

  .rows {
    font-variant-numeric: tabular-nums;
  }

  .link {
    border: 0;
    padding: 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-control);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .link:hover {
    background: var(--color-control-hover);
  }

  .link:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>
