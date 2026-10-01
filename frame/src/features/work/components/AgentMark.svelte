<script lang="ts">
  import type { NodeProps, Node } from "@xyflow/svelte";
  import { Character, type Mood } from "$shared/ui/presence";
  import { easing, reducedMotion } from "$shared/lib/motion";
  import type { CanvasItem } from "../lib/canvas-model";

  let { data, positionAbsoluteX, positionAbsoluteY }: NodeProps<Node<CanvasItem, "agent">> =
    $props();

  const MOODS: Record<string, Mood> = {
    thinking: "thinking",
    searching: "searching",
    reading: "reading",
    working: "working",
    writing: "working",
    done: "done",
  };
  const mood = $derived<Mood>(
    data.agent?.activity === "waiting_for_human"
      ? "waiting"
      : (MOODS[data.agent?.doing ?? ""] ?? "thinking"),
  );

  let across = $state<HTMLElement>();
  let down = $state<HTMLElement>();
  let ring = $state<HTMLElement>();
  let last: { x: number; y: number } | undefined;
  let legs: Animation[] = [];
  /** Where a walk in flight has the mark now, so a new one starts from there. */
  const offset = (element: HTMLElement) => {
    const matrix = new DOMMatrixReadOnly(getComputedStyle(element).transform);
    return { x: matrix.m41, y: matrix.m42 };
  };

  /**
   * The lead walks the way the lines run: along first, then down to the part,
   * and a ring opens where it arrives. The node itself jumps; only these two
   * nested layers travel, by transform alone.
   */
  $effect(() => {
    const x = positionAbsoluteX;
    const y = positionAbsoluteY;
    const from = last;
    last = { x, y };
    if (!from || !across || !down || !ring) return;
    const dx = from.x - x + offset(across).x;
    const dy = from.y - y + offset(down).y;
    for (const leg of legs) leg.cancel();
    legs = [];
    if ((Math.abs(dx) < 1 && Math.abs(dy) < 1) || reducedMotion()) return;
    const time = Math.min(1100, Math.max(560, 380 + Math.hypot(dx, dy) * 0.45));
    const curve = easing("in-out");
    legs = [
      across.animate([{ transform: `translateX(${dx}px)` }, { transform: "translateX(0)" }], {
        duration: time * 0.72,
        easing: curve,
        fill: "backwards",
      }),
      down.animate([{ transform: `translateY(${dy}px)` }, { transform: "translateY(0)" }], {
        duration: time * 0.72,
        delay: time * 0.28,
        easing: curve,
        fill: "backwards",
      }),
      ring.animate(
        [
          { transform: "scale(0.5)", opacity: 0.5 },
          { transform: "scale(2)", opacity: 0 },
        ],
        { duration: 720, delay: time * 0.9, easing: easing("out") },
      ),
    ];
  });
</script>

<div class="mark" role="status" aria-label={data.status}>
  <div class="across" bind:this={across}>
    <div class="down" bind:this={down}>
      <span class="ring" bind:this={ring}></span>
      <Character kind="lead" {mood} size={28} grounded />
    </div>
  </div>
</div>

<style>
  .mark {
    position: relative;
    inline-size: 24px;
    block-size: 24px;
    pointer-events: none;
  }

  .across,
  .down {
    position: absolute;
    inset: -2px;
  }

  .ring {
    position: absolute;
    inset: 0;
    border: 1px solid var(--color-agent-lead-sheen);
    border-radius: 50%;
    opacity: 0;
  }

  /* The mark travels inside its node, so the node itself only fades. */
  /* stylelint-disable-next-line selector-class-pattern */
  :global(.work-canvas .svelte-flow__node.svelte-flow__node-agent) {
    transition: opacity var(--motion-slow) var(--ease-exit);
  }
</style>
