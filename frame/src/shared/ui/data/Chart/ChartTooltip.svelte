<script lang="ts">
  import type { Snippet } from "svelte";
  import { Tooltip } from "layerchart/svg";
  import { duration } from "$shared/lib/motion";
  import type { ChartEvidence } from "./chart";
  import type { Readout as Reading } from "./layer";
  import Readout from "./Readout.svelte";

  /** shadcn-svelte's ChartTooltip, vendored: LayerChart's tooltip carrying the chart's card. */
  let {
    read,
    still = false,
    indicator = "dot",
    onevidence,
    glyph,
  }: {
    /** The readout for what the pointer is on. */
    read: (data: unknown) => Reading | null;
    still?: boolean;
    indicator?: "dot" | "line";
    onevidence?: (reference: ChartEvidence) => void;
    glyph?: Snippet<[ChartEvidence]>;
  } = $props();
</script>

<Tooltip.Root
  variant="none"
  portal={false}
  pointerEvents
  motion={still ? "none" : "spring"}
  fadeDuration={still ? 0 : duration("fast")}
>
  {#snippet children({ data })}
    {@const readout = read(data)}
    {#if readout}<Readout {readout} {indicator} {onevidence} {glyph} />{/if}
  {/snippet}
</Tooltip.Root>
