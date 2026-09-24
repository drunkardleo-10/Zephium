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
  const COLUMNS = 3;
  const ROWS = 3;
  const pictures = getContext<{ readonly map: ReadonlyMap<string, ComparePicture> }>(
    canvasPictures,
  );
  const model = $derived.by(() => {
    const content = item.artifact?.content;
    return content?.kind === "matrix" ? compareModel(content, pictures?.map) : null;
  });
  /** The card shows who is compared and the first things they differ on; the lift has all. */
  const columns = $derived(model?.columns.slice(0, COLUMNS) ?? []);
  const rows = $derived(model?.rows.slice(0, ROWS) ?? []);
  const rest = $derived(Math.max(0, (model?.columns.length ?? 0) - COLUMNS));
</script>

<CardFrame title={item.title} icon={GitCompareIcon} {selected} dense>
  {#if model}
    <div class="compare" style:--columns={columns.length} class:stub={rest > 0}>
      <div class="head">
        <span></span>
        {#each columns as column (column.key)}
          <div class="subject">
            <span class="picture"
              ><SubjectPicture picture={column.picture} name={column.name} /></span
            >
            <span class="name" title={column.name}>{column.name}</span>
            <span class="price"
              >{#if column.best}<span
                  class="best"
                  role="img"
                  title={m.work_compare_lowest()}
                  aria-label={m.work_compare_lowest()}
                ></span>{/if}<span>{column.price ?? ""}</span></span
            >
          </div>
        {/each}
        {#if rest}<span class="more" title={m.work_card_more({ count: rest })}>+{rest}</span>{/if}
      </div>
      <dl class="rows">
        {#each rows as row (row.key)}
          <div class="row">
            <dt title={row.label}>{row.label}</dt>
            {#each row.cells.slice(0, COLUMNS) as cell (cell.subject)}
              <dd class:numeric={row.numeric} class:check={row.check}>
                {#if cell.value.kind === "unknown"}<span class="dash">—</span>
                {:else if cell.value.kind === "mark"}<span
                    class="glyph"
                    class:yes={cell.value.yes}
                    aria-label={cell.value.yes ? m.work_yes() : m.work_no()}
                    ><Icon icon={cell.value.yes ? Tick02Icon : Cancel01Icon} size={13} /></span
                  >
                {:else}{#if cell.best}<span
                      class="best"
                      role="img"
                      title={m.work_compare_best()}
                      aria-label={m.work_compare_best()}
                    ></span>{/if}<span title={cell.value.text}>{cell.value.text}</span>{/if}
              </dd>
            {/each}
            {#if rest}<span></span>{/if}
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

    --grid: 88px repeat(var(--columns), minmax(0, 1fr));
  }

  .compare.stub {
    --grid: 88px repeat(var(--columns), minmax(0, 1fr)) 28px;
  }

  .head,
  .row {
    display: grid;
    grid-template-columns: var(--grid);
    align-items: start;
    gap: 10px;
    margin: 0;
  }

  .subject {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .picture {
    display: block;
    inline-size: 64px;
    block-size: 64px;
    margin-block-end: 4px;
    border-radius: var(--radius-sm);
    background: var(--color-fill);
    overflow: hidden;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-label);
    font-weight: 600;
    line-height: 16px;
  }

  .price {
    min-block-size: 16px;
    font-size: var(--text-label);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    line-height: 16px;
  }

  .more {
    place-self: center;
    padding: 0 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
    line-height: 18px;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 0;
    margin: 0;
    min-block-size: 0;
    overflow: hidden;
  }

  .row {
    padding-block: 5px;
    border-block-start: 1px solid var(--color-border);
  }

  dt,
  dd {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-label);
    line-height: 16px;
  }

  dt {
    color: var(--color-muted);
  }

  dd {
    margin: 0;
  }

  dd.numeric {
    font-variant-numeric: tabular-nums;
  }

  dd.check {
    text-align: center;
  }

  .best {
    display: inline-block;
    inline-size: 5px;
    block-size: 5px;
    margin-inline-end: 5px;
    border-radius: 50%;
    background: var(--color-accent);
    vertical-align: 0.15em;
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
    font-size: var(--text-label);
  }
</style>
