<script lang="ts" module>
  import type { CanvasPosition } from "../lib/canvas-model";
  type Side = "top" | "right" | "bottom" | "left";
  const OUT: Record<Side, readonly [number, number]> = {
    top: [0, -1],
    right: [1, 0],
    bottom: [0, 1],
    left: [-1, 0],
  };
  /** A tie the person drew between their own things: a cubic along its handles. */
  function curve(sx: number, sy: number, from: Side, tx: number, ty: number, to: Side) {
    const upright = from === "top" || from === "bottom";
    const along = Math.max(24, (upright ? Math.abs(ty - sy) : Math.abs(tx - sx)) / 2);
    const [ax, ay] = OUT[from];
    const [bx, by] = OUT[to];
    return `M ${sx},${sy} C ${sx + ax * along},${sy + ay * along} ${tx + bx * along},${ty + by * along} ${tx},${ty}`;
  }
  /** Between two ends that no longer stand where the run put them: across, down, across. */
  function elbow(from: CanvasPosition, to: CanvasPosition): CanvasPosition[] {
    if (Math.abs(from.y - to.y) < 1) return [from, to];
    if (Math.abs(from.x - to.x) < 1) return [from, to];
    const x = to.x > from.x + 24 ? Math.round((from.x + to.x) / 2) : from.x + 24;
    return to.x > from.x + 24
      ? [from, { x, y: from.y }, { x, y: to.y }, to]
      : [
          from,
          { x, y: from.y },
          { x, y: (from.y + to.y) / 2 },
          { x: to.x - 24, y: (from.y + to.y) / 2 },
          { x: to.x - 24, y: to.y },
          to,
        ];
  }
</script>

<script lang="ts">
  import { getContext, onMount } from "svelte";
  import { useStore, ViewportPortal, type EdgeProps } from "@xyflow/svelte";
  import { watchStill } from "$shared/ui/presence/still";
  import { canvasArrival } from "../lib/canvas-context";
  import { duration, easing, reducedMotion, type Duration } from "$shared/lib/motion";
  import { roundedPath } from "../lib/run/lines";
  type Tone = "rest" | "thread" | "flow" | "relation";
  type Route = {
    points: readonly CanvasPosition[];
    from: CanvasPosition;
    to: CanvasPosition;
    laid: { source: CanvasPosition; target: CanvasPosition };
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
  const route = $derived(data?.route as Route | undefined);
  const store = useStore();
  /** Where a node stands now; read again whenever any node moves. */
  const corner = (id: string) => {
    void store.nodes;
    return store.nodeLookup.get(id)?.internals.positionAbsolute;
  };
  /**
   * A run's line follows its route while both its ends stand where the run
   * put them, and moves with them when they move together; once the person
   * moves one alone, it takes an elbow between where they stand now.
   */
  const points = $derived.by(() => {
    if (!route) return null;
    const a = corner(source);
    const b = corner(target);
    if (!a || !b) return route.points;
    const da = { x: a.x - route.laid.source.x, y: a.y - route.laid.source.y };
    const db = { x: b.x - route.laid.target.x, y: b.y - route.laid.target.y };
    const still = (d: CanvasPosition) => Math.abs(d.x) < 0.5 && Math.abs(d.y) < 0.5;
    if (still(da) && still(db)) return route.points;
    if (Math.abs(da.x - db.x) < 0.5 && Math.abs(da.y - db.y) < 0.5)
      return route.points.map((point) => ({ x: point.x + da.x, y: point.y + da.y }));
    return elbow(
      { x: a.x + route.from.x, y: a.y + route.from.y },
      { x: b.x + route.to.x, y: b.y + route.to.y },
    );
  });
  const path = $derived(
    points
      ? roundedPath(points)
      : curve(sourceX, sourceY, `${sourcePosition}`, targetX, targetY, `${targetPosition}`),
  );
  let line = $state<SVGPathElement>();
  let pulse = $state<HTMLElement>();
  const carrying = $derived(!!data?.live && tone === "flow");
  /**
   * A line at work carries a pulse of light from its start to its end: one
   * element moved by transform along points sampled from the path, so it runs
   * on the compositor. It holds still off screen, while the window is hidden
   * and under reduced motion, and is gone once the part is done.
   */
  $effect(() => {
    const element = pulse;
    const d = path;
    if (!element || !carrying) return;
    const probe = document.createElementNS("http://www.w3.org/2000/svg", "path");
    probe.setAttribute("d", d);
    const length = probe.getTotalLength();
    if (length < 24) return;
    const steps = Math.max(8, Math.min(48, Math.round(length / 16)));
    const travel = 0.72;
    const keys: { offset: number; transform: string; opacity: number }[] = [];
    for (let index = 0; index <= steps; index++) {
      const at = (index / steps) * length;
      const point = probe.getPointAtLength(at);
      const ahead = probe.getPointAtLength(Math.min(length, at + 1));
      const behind = probe.getPointAtLength(Math.max(0, at - 1));
      const angle = (Math.atan2(ahead.y - behind.y, ahead.x - behind.x) * 180) / Math.PI;
      const edge = index === 0 || index === steps;
      keys.push({
        offset: (index / steps) * travel,
        transform: `translate(${point.x}px, ${point.y}px) rotate(${angle}deg)`,
        opacity: edge ? 0 : 1,
      });
    }
    keys.push({ offset: 1, transform: keys.at(-1)!.transform, opacity: 0 });
    const run = element.animate(keys, {
      duration: Math.min(2600, Math.max(1100, length * 3.2)) / travel,
      iterations: Infinity,
      easing: "linear",
    });
    run.pause();
    const stop = watchStill(element, (still) => (still ? run.pause() : run.play()));
    return () => {
      stop();
      run.cancel();
    };
  });
  // A line whose part just began draws itself in from where it leaves.
  onMount(() => {
    if (tone === "relation" || !line || !arrival?.(target)) return;
    if (reducedMotion()) {
      line.animate([{ opacity: 0 }, { opacity: 1 }], {
        duration: duration("fast"),
        easing: easing("out"),
      });
      return;
    }
    line.animate([{ strokeDashoffset: "1" }, { strokeDashoffset: "0" }], {
      duration: duration("page"),
      easing: easing("emphasized"),
    });
  });
</script>

<path
  bind:this={line}
  class="work-edge-line {tone}"
  class:live={!!data?.live}
  d={path}
  pathLength="1"
  stroke-dasharray={tone === "relation" ? undefined : "1"}
  fill="none"
/>
{#if carrying}<ViewportPortal target="back"
    ><span class="pulse" bind:this={pulse} aria-hidden="true"></span></ViewportPortal
  >{/if}

<style>
  .work-edge-line {
    stroke: var(--color-border-strong);
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: stroke var(--motion-base) var(--ease-out);
  }

  /* A run's line: one hairline, quieter than anything it joins. */
  .work-edge-line.flow,
  .work-edge-line.thread {
    stroke: var(--color-border-strong);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }

  .work-edge-line.thread {
    stroke: var(--color-border);
  }

  /* A line into a part at work is lit, and still: the part itself says what it does. */
  .work-edge-line.flow.live {
    stroke: color-mix(in oklab, var(--color-soft-sky) 55%, var(--color-border-strong));
  }

  /* The light a line at work carries: a short comet, its head brightest. */
  .pulse {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    inline-size: 44px;
    block-size: 3px;
    margin: -1.5px 0 0 -44px;
    border-radius: var(--radius-capsule);
    background: linear-gradient(
      90deg,
      transparent,
      color-mix(in oklab, var(--color-soft-sky) 80%, transparent) 70%,
      color-mix(in oklab, var(--color-soft-sky) 45%, var(--color-agent-light))
    );
    opacity: 0;
    transform-origin: 100% 50%;
    pointer-events: none;
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
