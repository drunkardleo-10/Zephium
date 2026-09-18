<script lang="ts">
  import { getContext } from "svelte";
  import CardFrame from "./CardFrame.svelte";
  import Icon from "$shared/ui/Icon";
  import SubjectPicture from "./SubjectPicture.svelte";
  import { Cancel01Icon, GitCompareIcon, Tick02Icon } from "../../lib/icons";
  import { canvasPictures } from "../../lib/canvas-context";
  import { compareModel, type ComparePicture } from "../../lib/compare";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const pictures = getContext<{ readonly map: ReadonlyMap<string, ComparePicture> }>(
    canvasPictures,
  );
  const model = $derived.by(() => {
    const content = item.artifact?.content;
    return content?.kind === "matrix" ? compareModel(content, pictures?.map) : null;
  });
  /** The card says who is compared and the first two things they differ on. */
  const rows = $derived(model?.rows.slice(0, 2) ?? []);
</script>

<CardFrame kind={item.kind} title={item.title} icon={GitCompareIcon} {selected} dense>
  {#if model}
    <div class="compare">
      <ul class="subjects">
        {#each model.columns.slice(0, 4) as column (column.key)}
          <li>
            <span class="picture">
              <SubjectPicture picture={column.picture} name={column.name} />
            </span>
            <span class="name">{column.name}</span>
            {#if column.price}<span class="price">{column.price}</span>{/if}
          </li>
        {/each}
      </ul>
      <dl class="rows">
        {#each rows as row (row.key)}
          <div class="row">
            <dt>{row.label}</dt>
            {#each row.cells.slice(0, 4) as cell (cell.subject)}
              <dd class:numeric={row.numeric}>
                {#if cell.value.kind === "unknown"}<span class="dash">—</span>
                {:else if cell.value.kind === "mark"}<span
                    class="glyph"
                    class:yes={cell.value.yes}
                    aria-label={cell.value.yes ? m.work_yes() : m.work_no()}
                    ><Icon icon={cell.value.yes ? Tick02Icon : Cancel01Icon} size={13} /></span
                  >
                {:else}{cell.value.text}{/if}
              </dd>
            {/each}
          </div>
        {/each}
      </dl>
    </div>
  {:else}<p class="summary">{item.detail || item.status}</p>{/if}
  {#snippet footer()}<span>{item.status}</span>{/snippet}
</CardFrame>

<style>
  .compare {
    display: flex;
    flex-direction: column;
    gap: 8px;
    block-size: 100%;
    min-block-size: 0;
    overflow: hidden;

    --compare-columns: repeat(4, minmax(0, 1fr));
  }

  .subjects,
  .row {
    display: grid;
    grid-template-columns: 78px var(--compare-columns);
    align-items: start;
    gap: 8px;
    margin: 0;
    padding: 0;
  }

  .subjects {
    list-style: none;
    grid-column: 1 / -1;
    grid-template-columns: 78px var(--compare-columns);
  }

  .subjects::before {
    content: "";
  }

  .subjects li {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-inline-size: 0;
  }

  .picture {
    inline-size: 100%;
    aspect-ratio: 1;
    max-block-size: 54px;
    border-radius: var(--radius-xs);
    background: var(--color-fill);
    overflow: hidden;
  }

  .name,
  dd {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-caption);
  }

  .name {
    font-weight: 600;
  }

  .price {
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    min-block-size: 0;
    overflow: hidden;
  }

  dt {
    color: var(--color-muted);
    font-size: var(--text-caption);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  dd {
    margin: 0;
  }

  dd.numeric {
    font-variant-numeric: tabular-nums;
  }

  .dash {
    color: var(--color-faint);
  }

  .glyph {
    display: inline-grid;
    place-items: center;
    color: var(--color-faint);
  }

  .glyph.yes {
    color: var(--color-success);
  }

  .summary {
    margin: 0;
    color: var(--color-muted);
  }
</style>
