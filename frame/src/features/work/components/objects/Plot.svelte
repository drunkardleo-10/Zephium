<script lang="ts">
  import type { Component } from "svelte";
  import { loadChart } from "$shared/ui/data/Chart";
  import type { PlotView } from "../../lib/board/types";
  import { watchNear } from "./near";
  import Title from "./Title.svelte";
  let {
    object,
    centre = false,
  }: {
    object: PlotView;
    /** Opened in the centre: the chart with its exact values. */
    centre?: boolean;
  } = $props();
  type ChartComponent = Awaited<ReturnType<typeof loadChart>>["default"];
  let Chart = $state.raw<ChartComponent | null>(null);
  let root = $state<HTMLElement>();
  let near = $state(false);
  $effect(() => (root ? watchNear(root, (value) => (near = value)) : undefined));
  $effect(() => {
    if (!near || Chart) return;
    let current = true;
    void loadChart().then((module) => {
      if (current) Chart = module.default as ChartComponent;
    });
    return () => (current = false);
  });
  // The exact values are read in the centre view; the canvas shows the chart.
  const spec = $derived(centre ? object.spec : { ...object.spec, values: false as const });
  const Drawn = $derived(Chart as Component<Record<string, unknown>> | null);
</script>

<section class="plot" bind:this={root} aria-label={object.title}>
  {#if object.title}<Title text={object.title} />{/if}
  <div class="figure">
    {#if near && Drawn}<Drawn title={object.title ?? ""} {spec} />{/if}
  </div>
</section>

<style>
  .plot {
    display: flex;
    flex-direction: column;
    gap: 14px;
    box-sizing: border-box;
    inline-size: 100%;
    padding: 20px 22px 18px;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
  }

  .figure {
    min-block-size: 120px;
  }
</style>
