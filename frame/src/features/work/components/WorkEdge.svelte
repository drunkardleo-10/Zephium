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
  /** The stretches: the wrapper has no box, so visibility is read from the first. */
  const pieces = $state<HTMLElement[]>([]);
  const carrying = $derived(!!data?.live && tone === "flow" && !!points);
  /** A dash and its gap: the work flows by this much each beat. */
  const PERIOD = 12;
  /**
   * A line at work carries fine dashes flowing from its start to its end.
   * Each straight stretch (its elbows left clear) is a window over a strip
   * of dashes that slides by one period, forever: transform only, on the
   * compositor. It holds still off screen, hidden, or under reduced motion.
   */
  const stretches = $derived.by(() => {
    if (!carrying || !points) return [];
    const list = points.filter(
      (point, index) =>
        index === 0 || point.x !== points[index - 1]!.x || point.y !== points[index - 1]!.y,
    );
    const out: { x: number; y: number; length: number; angle: number }[] = [];
    for (let index = 1; index < list.length; index++) {
      const a = list[index - 1]!;
      const b = list[index]!;
      const length = Math.hypot(b.x - a.x, b.y - a.y);
      const lead = index > 1 ? Math.min(12, length / 2) : 0;
      const tail = index < list.length - 1 ? Math.min(12, length / 2) : 0;
      const shown = length - lead - tail;
      if (shown < PERIOD * 2) continue;
      const ux = (b.x - a.x) / length;
      const uy = (b.y - a.y) / length;
      out.push({
        x: a.x + ux * lead,
        y: a.y + uy * lead,
        length: shown,
        angle: (Math.atan2(uy, ux) * 180) / Math.PI,
      });
    }
    return out;
  });
  let still = $state(true);
  $effect(() => {
    const element = pieces[0];
    if (!element) return;
    return watchStill(element, (next) => (still = next));
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
{#if stretches.length}<ViewportPortal target="back"
    ><span
      class="flow"
      data-still={still ? "" : undefined}
      style:--period="{PERIOD}px"
      aria-hidden="true"
      >{#each stretches as stretch, index (index)}<span
          class="stretch"
          bind:this={pieces[index]}
          style:inline-size="{stretch.length}px"
          style:transform="translate({stretch.x}px, {stretch.y}px) rotate({stretch.angle}deg)"
          ><span class="dashes"></span></span
        >{/each}</span
    ></ViewportPortal
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
    stroke: color-mix(in oklab, var(--color-text) 22%, var(--color-border-strong));
  }

  /* The work a line carries: fine dashes flowing along each straight stretch. */
  .flow {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    pointer-events: none;
  }

  .stretch {
    position: absolute;
    inset-block-start: -1px;
    inset-inline-start: 0;
    block-size: 2px;
    overflow: hidden;
    transform-origin: 0 50%;
  }

  .dashes {
    position: absolute;
    inset-block: 0;
    inset-inline: calc(var(--period) * -1) 0;
    background: repeating-linear-gradient(
      90deg,
      transparent 0,
      color-mix(in oklab, var(--color-text) 75%, transparent) 2px 4px,
      transparent 6px var(--period)
    );
    animation: flow 720ms linear infinite;
  }

  .flow[data-still] .dashes {
    animation-play-state: paused;
  }

  @keyframes flow {
    to {
      transform: translateX(var(--period));
    }
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
