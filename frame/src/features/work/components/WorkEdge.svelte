<script lang="ts" module>
  type Side = "top" | "right" | "bottom" | "left";
  const OUT: Record<Side, readonly [number, number]> = {
    top: [0, -1],
    right: [1, 0],
    bottom: [0, 1],
    left: [-1, 0],
  };
  /** A cubic that leaves and lands along its handles: level for a lane, upright for the thread. */
  function laneCurve(sx: number, sy: number, from: Side, tx: number, ty: number, to: Side) {
    const upright = from === "top" || from === "bottom";
    const along = Math.max(24, (upright ? Math.abs(ty - sy) : Math.abs(tx - sx)) / 2);
    const [ax, ay] = OUT[from];
    const [bx, by] = OUT[to];
    return `M ${sx},${sy} C ${sx + ax * along},${sy + ay * along} ${tx + bx * along},${ty + by * along} ${tx},${ty}`;
  }
</script>

<script lang="ts">
  import { getContext, onMount } from "svelte";
  import { EdgeLabel, useStore, type EdgeProps } from "@xyflow/svelte";
  import { canvasArrival } from "../lib/canvas-context";
  import { duration, easing, reducedMotion, type Duration } from "$shared/lib/motion";
  import { PLATE, arrowHead, elbow, flowPath, midpoint, plateHeight } from "../lib/diagram";
  import type { CanvasPosition } from "../lib/canvas-model";
  type Tone = "rest" | "thread" | "flow" | "relation" | "diagram";
  /** A diagram flow as its layout drew it, from its parts' corners in the layout. */
  type Flow = {
    points: CanvasPosition[];
    from: CanvasPosition;
    to: CanvasPosition;
    plate?: CanvasPosition;
    settled: boolean;
  };
  let {
    source,
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
  /** Work moving along a band's line: a dot travels it, and only then. */
  const moving = $derived(tone === "flow" && !!data?.live && !reducedMotion());
  const label = $derived(typeof data?.label === "string" ? data.label : "");
  /** At rest a flow is drawn whole or quiet; a looked-at part lights its own. */
  const mode = $derived((data?.state as "rest" | "lit" | "quiet" | undefined) ?? "rest");
  const flow = $derived(data?.flow as Flow | undefined);
  const tall = $derived(!!label && plateHeight(label) > PLATE.height);
  const store = useStore();
  /** A part as the canvas holds it now; read again whenever any node moves. */
  const internal = (id: string) => {
    void store.nodes;
    return store.nodeLookup.get(id);
  };
  /**
   * A diagram flow runs along the layout's line while both its parts stand
   * where the layout put them; once the person moves one, an elbow between
   * where they stand now.
   */
  const drawn = $derived.by(() => {
    if (tone !== "diagram") return null;
    const a = internal(source);
    const b = internal(target);
    if (!a || !b) return null;
    const pa = a.internals.positionAbsolute;
    const pb = b.internals.positionAbsolute;
    if (flow) {
      const dx = pa.x - flow.from.x;
      const dy = pa.y - flow.from.y;
      if (Math.abs(pb.x - flow.to.x - dx) < 0.5 && Math.abs(pb.y - flow.to.y - dy) < 0.5) {
        const move = (point: CanvasPosition) => ({ x: point.x + dx, y: point.y + dy });
        const points = flow.points.map(move);
        return { points, plate: flow.plate ? move(flow.plate) : midpoint(points) };
      }
    }
    const size = (node: typeof a) => ({
      width: node.measured.width ?? node.width ?? 220,
      height: node.measured.height ?? node.height ?? 72,
    });
    const points = elbow({ ...pa, ...size(a) }, { ...pb, ...size(b) });
    return { points, plate: midpoint(points) };
  });
  const path = $derived(
    drawn
      ? flowPath(drawn.points, 5)
      : laneCurve(sourceX, sourceY, `${sourcePosition}`, targetX, targetY, `${targetPosition}`),
  );
  let line = $state<SVGPathElement>();
  let dot = $state<SVGElement>();
  let group = $state<SVGGElement>();
  // A group that just arrived is reached by its edge, drawn from where it comes from.
  onMount(() => {
    if (tone === "relation" || tone === "diagram" || !line || !dot || !arrival?.(target)) return;
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
  // The engine's answer lands while the parts slide to it: the line fades in behind them.
  let wasSettled = false;
  $effect(() => {
    const now = !!flow?.settled;
    if (now && !wasSettled && group && !reducedMotion())
      group.animate([{ opacity: 0 }, { opacity: 1 }], {
        duration: duration("base"),
        easing: easing("emphasized"),
      });
    wasSettled = now;
  });
</script>

{#if drawn}<g bind:this={group} class="work-flow {mode}">
    <path bind:this={line} class="work-flow-line" d={path} fill="none" />
    <path bind:this={dot} class="work-flow-head" d={arrowHead(drawn.points, 5)} />
  </g>
  {#if label && mode !== "quiet"}<EdgeLabel
      x={drawn.plate.x}
      y={drawn.plate.y}
      class={["work-edge-label", { tall, lit: mode === "lit" }]}
      title={label}>{label}</EdgeLabel
    >{/if}
{:else if tone !== "diagram"}<path
    bind:this={line}
    class="work-edge-line {tone}"
    d={path}
    pathLength="1"
    stroke-dasharray={tone === "relation" ? undefined : "1"}
    fill="none"
  />
  <circle bind:this={dot} class="work-edge-dot {tone}" cx={targetX} cy={targetY} r="2" />
  {#if moving}<circle class="work-edge-pulse" r="2.5"
      ><animateMotion dur="1.8s" repeatCount="indefinite" {path} /></circle
    >{/if}{/if}

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

  /* A band's line: one hairline, quieter than anything it joins. */
  .work-edge-line.flow {
    stroke: var(--color-border);
    stroke-width: 1;
  }

  .work-edge-dot.flow {
    fill: var(--color-border-strong);
  }

  .work-edge-pulse {
    fill: var(--color-accent);
  }

  /* A diagram's flow: an elbow line with an arrowhead; quiet ones wait at 35 %. */
  .work-flow {
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .work-flow.quiet {
    opacity: 0.35;
  }

  .work-flow-line {
    stroke: var(--color-border-strong);
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: stroke var(--motion-fast) var(--ease-out);
  }

  .work-flow-head {
    fill: var(--color-border-strong);
    transition: fill var(--motion-fast) var(--ease-out);
  }

  .work-flow.lit .work-flow-line {
    stroke: var(--color-label-secondary);
  }

  .work-flow.lit .work-flow-head {
    fill: var(--color-label-secondary);
  }

  /* A flow's name on a plate up to PLATE.max wide and two lines tall; the whole name is its title. */
  /* stylelint-disable-next-line selector-class-pattern */
  :global(.svelte-flow__edge-label.work-edge-label) {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    box-sizing: border-box;
    max-inline-size: 220px;
    padding: 1px 6px;
    overflow: hidden;
    overflow-wrap: anywhere;
    border-radius: var(--radius-capsule);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 15px;
    text-align: center;
    animation: work-plate-in var(--motion-fast) var(--ease-out);
  }

  /* stylelint-disable-next-line selector-class-pattern */
  :global(.svelte-flow__edge-label.work-edge-label.tall) {
    border-radius: var(--radius-inset);
  }

  /* stylelint-disable-next-line selector-class-pattern */
  :global(.svelte-flow__edge-label.work-edge-label.lit) {
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
    color: var(--color-text);
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

  /* A plate lives in the label layer, outside this component: its keyframes are global. */
  /* stylelint-disable-next-line keyframes-name-pattern */
  @keyframes -global-work-plate-in {
    from {
      opacity: 0;
    }
  }
</style>
