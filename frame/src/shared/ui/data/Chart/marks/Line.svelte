<script lang="ts">
  import type { Trace } from "../geometry";
  /** Lines and areas on a monotone curve; a null is a gap, and points show on hover. */
  let {
    traces,
    hovered = null,
    enter = false,
  }: { traces: readonly Trace[]; hovered?: number | null; enter?: boolean } = $props();
</script>

{#each traces as trace (trace.series)}
  {#if trace.area}
    <path class="area" class:enter d={trace.area} style:fill={trace.color} />
  {/if}
  <path class="line" class:enter d={trace.line} pathLength="1" style:stroke={trace.color} />
{/each}
{#if hovered !== null}
  {#each traces as trace (trace.series)}
    {#each trace.dots.filter((dot) => dot.index === hovered) as dot (dot.index)}
      <circle class="dot" cx={dot.x} cy={dot.y} r="4" style:fill={trace.color} />
    {/each}
  {/each}
{/if}

<style>
  .line {
    fill: none;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .line.enter {
    stroke-dasharray: 1;
    animation: draw var(--motion-slow) var(--ease-emphasized) both;
  }

  .area {
    fill-opacity: 0.1;
    stroke: none;
  }

  .area.enter {
    animation: arrive var(--motion-slow) var(--ease-emphasized) both;
  }

  .dot {
    stroke: var(--color-surface);
    stroke-width: 2;
    animation: arrive var(--motion-fast) var(--ease-smooth) both;
  }

  @keyframes draw {
    from {
      stroke-dashoffset: 1;
    }

    to {
      stroke-dashoffset: 0;
    }
  }

  @keyframes arrive {
    from {
      opacity: 0;
    }
  }
</style>
