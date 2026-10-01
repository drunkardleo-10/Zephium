<script lang="ts">
  import type { Snippet } from "svelte";
  import Readout from "./Readout.svelte";
  import type { ChartEvidence, ChartSpec } from "./chart";
  import { radar, readout, type Plan } from "./layer";
  let {
    spec,
    shape,
    detail,
    summary,
    settled,
    onevidence,
    glyph,
  }: {
    spec: ChartSpec;
    shape: Plan;
    detail: "full" | "overview" | "tile";
    summary: string;
    settled: boolean;
    onevidence?: (reference: ChartEvidence) => void;
    glyph?: Snippet<[ChartEvidence]>;
  } = $props();
  let width = $state(0);
  let height = $state(0);
  let active = $state<number | null>(null);
  const web = $derived(radar(spec));
  const labelled = $derived(detail === "full");
  /** Room for the spoke names around the web, when they are written. */
  const gutter = $derived(labelled ? 28 : detail === "overview" ? 8 : 2);
  const radius = $derived(Math.max(8, Math.min(width, height) / 2 - gutter));
  const cx = $derived(width / 2);
  const cy = $derived(height / 2);
  const at = (angle: number, reach: number) => ({
    x: cx + Math.sin(angle) * radius * reach,
    y: cy - Math.cos(angle) * radius * reach,
  });
  const ring = (reach: number) =>
    web.axes
      .map((axis) => at(axis.angle, reach))
      .map((p) => `${p.x},${p.y}`)
      .join(" ");
  const top = $derived(web.ticks.at(-1) ?? 1);
  function polygon(reach: readonly (number | null)[]) {
    return web.axes
      .map((axis, index) => at(axis.angle, reach[index] ?? 0))
      .map((p) => `${p.x},${p.y}`)
      .join(" ");
  }
  /** A wedge around each spoke catches the pointer for that category. */
  function wedge(index: number) {
    const half = Math.PI / Math.max(1, web.axes.length);
    const angle = web.axes[index]!.angle;
    const a = at(angle - half, 1.08);
    const b = at(angle + half, 1.08);
    return `M${cx},${cy} L${a.x},${a.y} L${b.x},${b.y} Z`;
  }
  const tip = $derived(active === null ? null : readout(spec, shape, active));
  const tipAt = $derived(active === null ? null : at(web.axes[active]!.angle, 0.9));
</script>

<div class="radar" bind:clientWidth={width} bind:clientHeight={height}>
  {#if width && height && web.axes.length >= 3}
    <svg {width} {height} role="img" aria-label={summary} class:enter={!settled}>
      {#each web.ticks as tick (tick)}
        <polygon points={ring(tick / top)} class="grid" />
      {/each}
      {#each web.axes as axis (axis.key)}
        {@const end = at(axis.angle, 1)}
        <line x1={cx} y1={cy} x2={end.x} y2={end.y} class="grid" />
      {/each}
      {#each web.series as series, order (order)}
        <polygon
          points={polygon(series.reach)}
          class="mark web"
          style:fill={series.color}
          style:stroke={series.color}
          style:fill-opacity={web.series.length > 1 ? 0.14 : 0.24}
        />
        {#if detail !== "tile" && web.series.length <= 2}
          {#each series.reach as reach, index (index)}
            {#if reach !== null}
              {@const p = at(web.axes[index]!.angle, reach)}
              <circle
                cx={p.x}
                cy={p.y}
                r={active === index ? 4.5 : 3.5}
                class="mark dot"
                style:fill={series.color}
              />
            {/if}
          {/each}
        {/if}
      {/each}
      {#if labelled}
        {#each web.axes as axis, index (axis.key)}
          {@const p = at(axis.angle, 1)}
          {@const sin = Math.sin(axis.angle)}
          {@const cos = Math.cos(axis.angle)}
          <text
            x={p.x + sin * 10}
            y={p.y - cos * 10}
            text-anchor={Math.abs(sin) < 0.2 ? "middle" : sin > 0 ? "start" : "end"}
            dominant-baseline={cos > 0.5 ? "auto" : cos < -0.5 ? "hanging" : "central"}
            class="name"
            class:on={active === index}>{axis.label}</text
          >
        {/each}
        {#each web.axes as _, index (index)}
          <path
            d={wedge(index)}
            class="hit"
            role="presentation"
            onpointerenter={() => (active = index)}
            onpointerleave={() => (active = null)}
          />
        {/each}
      {/if}
    </svg>
    {#if tip && tipAt}
      <div class="tip" style:left={`${tipAt.x}px`} style:top={`${tipAt.y}px`}>
        <Readout readout={tip} {onevidence} {glyph} />
      </div>
    {/if}
  {/if}
</div>

<style>
  .radar {
    position: relative;
    inline-size: 100%;
    block-size: 100%;
  }

  svg {
    display: block;
    overflow: visible;
  }

  .grid {
    fill: none;
    stroke: var(--color-border);
    stroke-width: 1;
  }

  .web {
    stroke-width: 1.5;
    stroke-linejoin: round;
  }

  .dot {
    stroke: var(--color-surface);
    stroke-width: 1.5;
  }

  .name {
    fill: var(--color-muted);
    font-size: var(--text-caption);
  }

  .name.on {
    fill: var(--color-text);
  }

  .hit {
    fill: transparent;
  }

  .enter {
    animation: arrive var(--motion-base) var(--ease-emphasized) both;
  }

  .tip {
    position: absolute;
    z-index: 1;
    transform: translate(-50%, -115%);
    pointer-events: none;
  }

  @keyframes arrive {
    from {
      opacity: 0;
    }
  }
</style>
