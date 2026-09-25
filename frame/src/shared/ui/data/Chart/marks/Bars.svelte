<script lang="ts">
  import type { Bar } from "../geometry";
  /** Bars, ranges, stacks and spark bars: each grows once from its own baseline. */
  let {
    bars,
    hovered = null,
    enter = false,
  }: { bars: readonly Bar[]; hovered?: number | null; enter?: boolean } = $props();
</script>

{#each bars as bar (`${bar.series}:${bar.index}`)}
  <path
    class="bar"
    class:enter
    class:across={bar.axis === "x"}
    class:dim={hovered !== null && hovered !== bar.index}
    d={bar.d}
    style:fill={bar.color}
    style:transform-origin={`${bar.origin.x}px ${bar.origin.y}px`}
  />
{/each}
{#each bars as bar (`${bar.series}:${bar.index}`)}
  {#if bar.value}
    <text class="value" class:enter x={bar.value.x} y={bar.value.y} text-anchor={bar.value.anchor}
      >{bar.value.text}</text
    >
  {/if}
{/each}

<style>
  .bar {
    opacity: 0.86;
    transform-box: view-box;
    transition: opacity var(--motion-fast) var(--ease-smooth);
  }

  .bar.dim {
    opacity: 0.4;
  }

  .bar.enter {
    animation: grow-up var(--motion-slow) var(--ease-emphasized) both;
  }

  .bar.enter.across {
    animation-name: grow-across;
  }

  .value {
    fill: var(--color-muted);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }

  .value.enter {
    animation: arrive var(--motion-slow) var(--ease-emphasized) both;
  }

  @keyframes grow-up {
    from {
      transform: scaleY(0);
    }
  }

  @keyframes grow-across {
    from {
      transform: scaleX(0);
    }
  }

  @keyframes arrive {
    from {
      opacity: 0;
    }
  }
</style>
