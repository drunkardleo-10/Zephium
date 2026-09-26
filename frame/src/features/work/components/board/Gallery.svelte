<script lang="ts">
  import { getContext } from "svelte";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import EntityCard from "./EntityCard.svelte";
  import Compare from "../compare/Compare.svelte";
  import { CARD, GALLERY, cardWidth } from "../../lib/board/size";
  import { canvasPictures, type BoardActions } from "../../lib/canvas-context";
  import { compareModel, type ComparePicture } from "../../lib/compare";
  import type { GalleryBlock } from "../../lib/board/types";
  import * as m from "$shared/i18n/messages";
  let {
    block,
    sources,
    actions,
    width,
    open = false,
  }: {
    block: GalleryBlock;
    sources: Readonly<Record<string, EvidenceReference>>;
    actions?: BoardActions;
    /** The room the cards share. */
    width: number;
    open?: boolean;
  } = $props();
  const pictures = getContext<{ readonly map: ReadonlyMap<string, ComparePicture> } | undefined>(
    canvasPictures,
  );
  const shown = $derived(open ? block.entities : block.entities.slice(0, GALLERY));
  const rest = $derived(block.entities.length - shown.length);
  /** Rows balance: six read three and three, never five and one. */
  const columns = $derived.by(() => {
    const fit = Math.max(1, Math.floor((width + CARD.gap) / (cardWidth(block) + CARD.gap)));
    const count = shown.length;
    const rows = Math.ceil(count / fit);
    return Math.max(1, Math.min(fit, Math.ceil(count / rows)));
  });
  const model = $derived(block.compare ? compareModel(block.compare, pictures?.map) : null);
</script>

<div class="gallery" style:--columns={columns} style:--gap={`${CARD.gap}px`}>
  {#each shown as entity (entity.key)}<EntityCard {entity} {sources} {actions} />{/each}
</div>
{#if open && model}<div class="compare nowheel">
    <Compare {model} onevidence={actions?.evidence} />
  </div>{/if}
{#if model || rest > 0 || (open && block.entities.length > GALLERY)}<footer>
    {#if rest > 0}<button
        type="button"
        class="action nodrag nopan"
        onclick={() => actions?.toggle(block.id)}
        >{m.work_board_show_all({ count: block.entities.length })}</button
      >{/if}
    {#if model && !open}<button
        type="button"
        class="action nodrag nopan"
        onclick={() => actions?.toggle(block.id)}>{m.work_board_compare()}</button
      >{/if}
    {#if open}<button
        type="button"
        class="action nodrag nopan"
        onclick={() => actions?.toggle(block.id)}>{m.work_board_fewer()}</button
      >{/if}
  </footer>{/if}

<style>
  .gallery {
    display: grid;
    grid-template-columns: repeat(var(--columns), minmax(0, 1fr));
    gap: 20px var(--gap);
    align-items: start;
  }

  .compare {
    overflow-x: auto;
    margin-block-start: 20px;
    padding-block-start: 16px;
    border-block-start: 1px solid var(--color-border);
  }

  footer {
    display: flex;
    gap: 6px;
    margin-block-start: 16px;
  }

  .action {
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

  .action:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>
