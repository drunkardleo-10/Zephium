<script lang="ts">
  import type { Snippet } from "svelte";
  import type { ChartEvidence } from "./chart";
  import type { Readout } from "./layer";
  /** The tooltip's content: the point, its value, and the sources behind it. */
  let {
    readout,
    onevidence,
    glyph,
  }: {
    readout: Readout;
    onevidence?: (reference: ChartEvidence) => void;
    glyph?: Snippet<[ChartEvidence]>;
  } = $props();
  const letter = (reference: ChartEvidence) =>
    (reference.origin || reference.label)
      .replace(/^www\./u, "")
      .slice(0, 1)
      .toLocaleUpperCase();
</script>

<div class="tip">
  <span class="title">{readout.title}</span>
  {#each readout.rows as row, index (index)}
    <span class="row">
      {#if row.name}<span class="swatch" style:background={row.color}></span><span class="name"
          >{row.name}</span
        >{/if}<span class="value" class:alone={!row.name}>{row.text}</span>
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
          {#if glyph}{@render glyph(reference)}{:else}<span class="letter" aria-hidden="true"
              >{letter(reference)}</span
            >{/if}
          <span class="host"
            >{reference.file ? reference.label : reference.origin || reference.label}</span
          >
        </button>
      {/each}
    </span>
  {/if}
</div>

<style>
  .tip {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-inline-size: 132px;
    max-inline-size: 260px;
    padding: 8px 10px;
    border-radius: var(--radius-control-compact);
    background: var(--color-float);
    box-shadow: var(--shadow-float);
    color: var(--color-text);
    font-size: var(--text-caption);
    line-height: 14px;
  }

  .title {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .swatch {
    flex: none;
    inline-size: 8px;
    block-size: 8px;
    border-radius: var(--radius-capsule);
  }

  .name {
    flex: 1;
    min-inline-size: 0;
    color: var(--color-muted);
  }

  .value {
    padding-inline-start: 12px;
    font-variant-numeric: tabular-nums;
    font-weight: 550;
  }

  .value.alone {
    padding: 0;
    font-size: var(--text-body);
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

  .letter {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 12px;
    block-size: 12px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-faint);
    font-size: 8px;
    font-weight: 600;
  }

  .host {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chip:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>
