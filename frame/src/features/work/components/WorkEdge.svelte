<script lang="ts">
  import { getContext, onMount } from "svelte";
  import { EdgeLabel, Position, type EdgeProps } from "@xyflow/svelte";
  import { canvasArrival } from "../lib/canvas-context";
  import { duration, easing, reducedMotion, type Duration } from "$shared/lib/motion";
  type Tone = "rest" | "thread" | "relation" | "diagram";
  let {
    target,
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
    data,
  }: EdgeProps = $props();
  const arrival = getContext<((id: string) => Duration | null) | undefined>(canvasArrival);
  const tone = $derived((data?.tone as Tone | undefined) ?? "relation");
  /** What flows along a diagram's edge, on a plate at its midpoint. */
  const label = $derived(typeof data?.label === "string" ? data.label : "");
  /** A cubic that leaves and lands along its handles: level for a lane, upright for the thread. */
  const path = $derived.by(() => {
    const vertical = sourcePosition === Position.Bottom || targetPosition === Position.Top;
    const reach = vertical
      ? Math.max(24, Math.abs(targetY - sourceY) / 2)
      : Math.max(24, Math.abs(targetX - sourceX) / 2);
    const [c1x, c1y, c2x, c2y] = vertical
      ? [sourceX, sourceY + reach, targetX, targetY - reach]
      : [sourceX + reach, sourceY, targetX - reach, targetY];
    return `M ${sourceX},${sourceY} C ${c1x},${c1y} ${c2x},${c2y} ${targetX},${targetY}`;
  });
  let line = $state<SVGPathElement>();
  let dot = $state<SVGCircleElement>();
  // A group that just arrived is reached by its edge, drawn from where it comes from.
  onMount(() => {
    if (tone === "relation" || !line || !dot || !arrival?.(target)) return;
    if (reducedMotion()) {
      for (const element of [line, dot])
        element.animate([{ opacity: 0 }, { opacity: 1 }], {
          duration: duration("fast"),
          easing: easing("out"),
        });
      return;
    }
    const slow = duration("slow");
    line.animate([{ strokeDashoffset: "1" }, { strokeDashoffset: "0" }], {
      duration: slow,
      easing: easing("emphasized"),
    });
    dot.animate([{ opacity: 0 }, { opacity: 1 }], {
      duration: duration("fast"),
      delay: slow * 0.8,
      easing: easing("out"),
      fill: "backwards",
    });
  });
</script>

<path
  bind:this={line}
  class="work-edge-line {tone}"
  d={path}
  pathLength="1"
  stroke-dasharray={tone === "relation" ? undefined : "1"}
  fill="none"
/>
<circle bind:this={dot} class="work-edge-dot {tone}" cx={targetX} cy={targetY} r="2" />
{#if label}<EdgeLabel
    x={(sourceX + targetX) / 2}
    y={(sourceY + targetY) / 2}
    class="work-edge-label">{label}</EdgeLabel
  >{/if}

<style>
  .work-edge-line {
    stroke: var(--color-border-strong);
    stroke-width: 1.5;
    stroke-linecap: round;
  }

  .work-edge-dot {
    fill: var(--color-border-strong);
  }

  .thread {
    opacity: 0.6;
  }

  /* A diagram's flow names itself on a plate, never over a card's words. */
  /* stylelint-disable-next-line selector-class-pattern */
  :global(.svelte-flow__edge-label.work-edge-label) {
    max-inline-size: 140px;
    padding: 1px 6px;
    overflow: hidden;
    border-radius: var(--radius-capsule);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 15px;
    text-overflow: ellipsis;
    white-space: nowrap;
    pointer-events: none !important;
  }

  /* A tie lit by a focused card comes and goes with the pointer. */
  .relation {
    animation: relation-in var(--motion-fast) var(--ease-out);
  }

  @keyframes relation-in {
    from {
      opacity: 0;
    }
  }
</style>
