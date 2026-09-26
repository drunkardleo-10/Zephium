<script lang="ts">
  import { getContext } from "svelte";
  import Compare from "../compare/Compare.svelte";
  import Table from "./Table.svelte";
  import { canvasPictures, type BoardActions } from "../../lib/canvas-context";
  import { compareModel, type ComparePicture } from "../../lib/compare";
  import { columnTypes } from "../../lib/board/tabular";
  import type { ComparisonBlock } from "../../lib/board/types";
  import * as m from "$shared/i18n/messages";
  let {
    block,
    actions,
    open = false,
  }: { block: ComparisonBlock; actions?: BoardActions; open?: boolean } = $props();
  const ROWS = 6;
  const pictures = getContext<{ readonly map: ReadonlyMap<string, ComparePicture> } | undefined>(
    canvasPictures,
  );
  const content = $derived(block.content);
  const model = $derived(content.kind === "matrix" ? compareModel(content, pictures?.map) : null);
  const shown = $derived(
    model && !open ? { ...model, rows: model.rows.slice(0, ROWS), notes: [] } : model,
  );
  const rest = $derived(model ? model.rows.length - ROWS : 0);
  /** A plain comparison is a table whose first column names the alternatives. */
  const table = $derived.by(() => {
    if (content.kind !== "comparison") return null;
    const columns = ["", ...content.criteria];
    const rows = content.alternatives.map((row) => [row.name, ...row.values]);
    return {
      id: block.id,
      kind: "table" as const,
      emphasis: block.emphasis,
      state: block.state,
      columns: columnTypes(columns, rows),
      rows,
    };
  });
</script>

{#if shown}
  <div class="compare nowheel" class:open>
    <Compare model={shown} onevidence={actions?.evidence} />
  </div>
  {#if rest > 0}<footer>
      <button type="button" class="more nodrag nopan" onclick={() => actions?.toggle(block.id)}
        >{open
          ? m.work_board_fewer()
          : m.work_board_all_criteria({ count: model!.rows.length })}</button
      >
    </footer>{/if}
{:else if table}<Table block={table} {actions} {open} />{/if}

<style>
  .compare {
    overflow-x: auto;
  }

  .open {
    max-block-size: 720px;
    overflow-y: auto;
  }

  footer {
    margin-block-start: 12px;
  }

  .more {
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

  .more:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>
