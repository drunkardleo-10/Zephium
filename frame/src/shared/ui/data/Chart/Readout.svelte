<script lang="ts">
  import type { Snippet } from "svelte";
  import type { ChartEvidence } from "./chart";
  import type { Readout } from "./layer";

  /** The tooltip's card, shadcn-svelte's: the point, a row per series with its indicator, and the sources behind it. */
  let {
    readout,
    indicator = "dot",
    onevidence,
    glyph,
  }: {
    readout: Readout;
    indicator?: "dot" | "line";
    onevidence?: (reference: ChartEvidence) => void;
    glyph?: Snippet<[ChartEvidence]>;
  } = $props();
</script>

<div class="chart-tooltip">
  <span class="label">{readout.title}</span>
  {#each readout.rows as row, index (index)}
    <span class="row">
      <span class="indicator {indicator}" style:--indicator={row.color}></span>
      {#if row.name}<span class="name">{row.name}</span>{/if}
      <span class="value" class:alone={!row.name}>{row.text}</span>
    </span>
  {/each}
  {#if readout.evidence.length}
    <span class="chips">
      {#each readout.evidence as reference (reference.key)}
        <button
          type="button"
          class="chip"
          disabled={!onevidence}
          title={reference.label}
          onclick={() => onevidence?.(reference)}
        >
          {#if glyph}{@render glyph(reference)}{/if}
          <span class="host"
            >{reference.file ? reference.label : reference.origin || reference.label}</span
          >
        </button>
      {/each}
    </span>
  {/if}
</div>

<style>
  .chart-tooltip {
    display: grid;
    gap: 6px;
    min-inline-size: 128px;
    max-inline-size: 260px;
    padding: 7px 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control-compact);
    background: var(--color-float);
    box-shadow: var(--shadow-float);
    color: var(--color-text);
    font-size: var(--text-caption);
    line-height: 14px;
  }

  .label {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .indicator {
    flex: none;
    background: var(--indicator);
  }

  .indicator.dot {
    inline-size: 10px;
    block-size: 10px;
    border-radius: var(--radius-swatch);
  }

  .indicator.line {
    inline-size: 4px;
    block-size: 14px;
    border-radius: var(--radius-capsule);
  }

  .name {
    flex: 1;
    min-inline-size: 0;
    color: var(--color-muted);
  }

  .value {
    margin-inline-start: auto;
    padding-inline-start: 12px;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-weight: 500;
  }

  .value.alone {
    margin-inline-start: 0;
    padding: 0;
    font-family: inherit;
    font-size: var(--text-body);
    font-weight: 600;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-block-start: 2px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-inline-size: 160px;
    padding: 1px 8px 1px 2px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font: inherit;
    line-height: 14px;
    cursor: default;
  }

  .chip:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .host {
    min-inline-size: 0;
    overflow-wrap: anywhere;
  }

  .chip:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>
