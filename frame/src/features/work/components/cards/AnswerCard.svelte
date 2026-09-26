<script lang="ts">
  import { getContext } from "svelte";
  import CardFrame from "./CardFrame.svelte";
  import AnswerView from "$shared/ui/data/Artifact/AnswerView.svelte";
  import EvidenceChips from "$shared/ui/data/Artifact/EvidenceChips.svelte";
  import { answerCover, type ArtifactContent } from "$shared/ui/data/Artifact/artifact";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import { canvasEvidence, canvasOpen } from "../../lib/canvas-context";
  import { answerCard } from "../../lib/card-size";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let {
    id,
    item,
    content,
    selected,
    onaction,
  }: {
    id: string;
    item: CanvasItem;
    content: Extract<ArtifactContent, { kind: "answer" }>;
    selected: boolean;
    onaction: () => void;
  } = $props();
  const open = getContext<((id: string) => void) | undefined>(canvasOpen);
  const evidence = getContext<{ open?: (id: string, reference: EvidenceReference) => void }>(
    canvasEvidence,
  );
  const cited = $derived(
    !item.artifact?.knowledge && item.artifact?.evidence.length ? item.artifact.evidence : [],
  );
  const cover = $derived(answerCover(content.blocks));
  /** The card holds less than the whole answer: it fades and offers the rest. */
  const more = $derived(
    answerCard(item.title, content.blocks, {
      action: !!item.actionLabel,
      cited: cited.length > 0,
    }).more,
  );
  // A press that moved was a drag; a control inside the card keeps its own click.
  let pressed: { x: number; y: number } | null = null;
  function openCover(event: MouseEvent) {
    const from = pressed;
    pressed = null;
    if (from && Math.hypot(event.clientX - from.x, event.clientY - from.y) > 4) return;
    if ((event.target as Element).closest("button:not(:disabled), a, input, select")) return;
    open?.(id);
  }
</script>

<!-- The reply's lead and first section; the lift reads it whole. -->
<CardFrame
  {id}
  kind={m.work_card_kind_answer()}
  title={item.title}
  {selected}
  footer={more || cited.length || item.actionLabel ? footer : undefined}
>
  <div
    class="answer-body"
    class:more
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
    <AnswerView blocks={cover} label={item.title} />
  </div>
</CardFrame>
{#snippet footer()}<span class="chips"
    >{#if cited.length}<EvidenceChips
        references={cited}
        compact
        onevidence={evidence?.open ? (reference) => evidence.open?.(id, reference) : undefined}
      />{/if}</span
  >{#if item.actionLabel}<button
      type="button"
      class="link nodrag nopan"
      onclick={(event) => {
        event.stopPropagation();
        onaction();
      }}>{item.actionLabel}</button
    >{:else if more}<button
      type="button"
      class="link nodrag nopan"
      onclick={(event) => {
        event.stopPropagation();
        open?.(id);
      }}>{m.work_answer_read()}</button
    >{/if}{/snippet}

<style>
  .answer-body {
    box-sizing: border-box;
    block-size: 100%;
    overflow: hidden;
    cursor: default;
  }

  /* The last lines give way to the card rather than stopping at a cut. */
  .answer-body.more {
    mask-image: linear-gradient(to bottom, black calc(100% - 40px), transparent);
  }

  .answer-body:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
    border-radius: var(--radius-inset);
  }

  .chips {
    display: flex;
    min-inline-size: 0;
    overflow: hidden;
  }

  .link {
    flex: none;
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
