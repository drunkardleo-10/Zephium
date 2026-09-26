<script lang="ts">
  import type { Snippet } from "svelte";
  import type { ChartEvidence } from "./chart";
  import type { Readout } from "./geometry";
  /** A plain popover: the point, its value, and the sources behind it. */
  let {
    readout,
    left,
    top,
    align,
    below,
    onevidence,
    glyph,
  }: {
    readout: Readout;
    /** Percentages of the plot box, so the popover follows the plot's own scale. */
    left: number;
    top: number;
    align: "start" | "middle" | "end";
    below: boolean;
    onevidence?: (reference: ChartEvidence) => void;
    glyph?: Snippet<[ChartEvidence]>;
  } = $props();
  const letter = (reference: ChartEvidence) =>
    (reference.origin || reference.label)
      .replace(/^www\./u, "")
      .slice(0, 1)
      .toLocaleUpperCase();
</script>

<div
  class="tip"
  class:start={align === "start"}
  class:end={align === "end"}
  class:below
  style:left={`${left}%`}
  style:top={`${top}%`}
>
  <span class="title">{readout.title}</span>
  {#each readout.rows as row, index (index)}
    <span class="row">
      {#if row.name}<span class="swatch" style:background={row.color}></span><span class="name"
          >{row.name}</span
        >{/if}<span class="value">{row.text}</span>
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
    position: absolute;
    z-index: 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
    max-inline-size: 240px;
    padding: 6px 10px;
    border-radius: var(--radius-control-compact);
    background: var(--color-float);
    box-shadow: var(--shadow-float);
    color: var(--color-text);
    font-size: var(--text-caption);
    transform: translate(-50%, calc(-100% - 8px));
    animation: arrive var(--motion-fast) var(--ease-smooth) both;
  }

  .tip.start {
    transform: translate(-12px, calc(-100% - 8px));
  }

  .tip.end {
    transform: translate(calc(-100% + 12px), calc(-100% - 8px));
  }

  .tip.below {
    translate: 0 calc(100% + 20px);
  }

  .title {
    color: var(--color-muted);
    overflow-wrap: anywhere;
  }

  .row {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .swatch {
    flex: none;
    inline-size: 8px;
    block-size: 8px;
    border-radius: var(--radius-capsule);
  }

  .name {
    color: var(--color-muted);
  }

  .value {
    font-variant-numeric: tabular-nums;
    font-weight: 550;
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

  @keyframes arrive {
    from {
      opacity: 0;
    }
  }

  .chip:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>
