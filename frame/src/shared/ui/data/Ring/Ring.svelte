<!--
  A progress ring. The arc moves by stroke offset with a short linear glide,
  so a caller that steps it once a second reads as continuous without the
  ring repainting every frame.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    value,
    size = 120,
    stroke = 8,
    tone = "lead",
    label,
    children,
  }: {
    /** Progress from 0 to 1. */
    value: number;
    size?: number;
    stroke?: number;
    /** `rest` for a break: the same ring, a calmer colour. */
    tone?: "lead" | "rest";
    label?: string;
    children?: Snippet;
  } = $props();

  let radius = $derived((size - stroke) / 2);
  let circumference = $derived(2 * Math.PI * radius);
  let offset = $derived(circumference * (1 - Math.min(1, Math.max(0, value))));
</script>

<div
  class="dial"
  class:rest={tone === "rest"}
  style:inline-size={`${size}px`}
  style:block-size={`${size}px`}
  role={label ? "progressbar" : undefined}
  aria-label={label}
  aria-valuemin={label ? 0 : undefined}
  aria-valuemax={label ? 100 : undefined}
  aria-valuenow={label ? Math.round(value * 100) : undefined}
>
  <svg viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
    <circle class="track" cx={size / 2} cy={size / 2} r={radius} stroke-width={stroke} />
    <circle
      class="arc"
      cx={size / 2}
      cy={size / 2}
      r={radius}
      stroke-width={stroke}
      stroke-dasharray={circumference}
      stroke-dashoffset={offset}
    />
  </svg>
  {#if children}<div class="center">{@render children()}</div>{/if}
</div>

<style>
  .dial {
    position: relative;
    display: inline-grid;
    place-items: center;
    flex: none;
  }

  svg {
    position: absolute;
    inset: 0;
    rotate: -90deg;
  }

  .track {
    fill: none;
    stroke: var(--color-track);
  }

  .arc {
    fill: none;
    stroke: var(--color-text);
    stroke-linecap: round;
    transition:
      stroke-dashoffset 1s linear,
      stroke var(--motion-slow) var(--ease-out);
  }

  .rest .arc {
    stroke: var(--color-soft-mint);
  }

  .center {
    position: relative;
    display: grid;
    place-items: center;
    text-align: center;
  }

  @media (prefers-reduced-motion: reduce) {
    .arc {
      transition: none;
    }
  }

  :global(:root[data-reduce-motion="true"]) .arc {
    transition: none;
  }

  @media (forced-colors: active) {
    .arc {
      stroke: Highlight;
    }

    .track {
      stroke: GrayText;
    }
  }
</style>
