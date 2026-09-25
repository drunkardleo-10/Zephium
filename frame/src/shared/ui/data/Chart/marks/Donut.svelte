<script lang="ts">
  import type { Anchor, Slice } from "../geometry";
  /** A share of a whole, with the whole stated in the middle. */
  let {
    slices,
    centre,
    total,
    caption,
    hovered = null,
    enter = false,
    onhover,
  }: {
    slices: readonly Slice[];
    centre: Anchor;
    total: string;
    caption: string;
    hovered?: number | null;
    enter?: boolean;
    onhover: (index: number) => void;
  } = $props();
</script>

<g transform={`translate(${centre.x},${centre.y})`}>
  <g class="ring" class:enter>
    {#each slices as slice (slice.index)}
      <path
        class="slice"
        role="presentation"
        class:dim={hovered !== null && hovered !== slice.index}
        d={slice.d}
        style:fill={slice.color}
        onpointerenter={() => onhover(slice.index)}
      />
    {/each}
  </g>
  <text class="total" y="2" text-anchor="middle">{total}</text>
  <text class="caption" y="16" text-anchor="middle">{caption}</text>
</g>

<style>
  .ring {
    transform-box: fill-box;
    transform-origin: center;
  }

  .ring.enter {
    animation: arrive var(--motion-slow) var(--ease-emphasized) both;
  }

  .slice {
    transition: opacity var(--motion-fast) var(--ease-smooth);
  }

  .slice.dim {
    opacity: 0.4;
  }

  .total {
    fill: var(--color-text);
    font-size: var(--text-body);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .caption {
    fill: var(--color-faint);
    font-size: 10px;
  }

  @keyframes arrive {
    from {
      opacity: 0;
      transform: scale(0.92);
    }
  }
</style>
