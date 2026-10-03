<!--
  Time per bucket as quiet columns. Each bar is a full-height plate slid down
  inside a clipped track, so it rises by transform alone and keeps its rounded
  cap at every height; a change of data glides instead of redrawing.
-->
<script lang="ts">
  import { bucketAt, gridLines, timeScale } from "./bars";

  let {
    values,
    highlight = null,
    live = -1,
    average = null,
    ticks = [],
    label,
    describe,
    format,
    height = 120,
    axis = true,
    selected = -1,
    onselect,
  }: {
    values: number[];
    /** A part of each bar to bring forward, such as one site's share. */
    highlight?: number[] | null;
    /** The bucket happening now. */
    live?: number;
    average?: number | null;
    /** Labels under chosen buckets. */
    ticks?: { index: number; label: string }[];
    /** Names the chart for assistive technology. */
    label: string;
    /** Names one bucket in the readout: "2 PM", "Tuesday". */
    describe: (index: number) => string;
    format: (seconds: number) => string;
    height?: number;
    /** Values along the trailing edge; off where the chart is narrow. */
    axis?: boolean;
    selected?: number;
    /** Choosing a bucket, by click or Enter. */
    onselect?: (index: number) => void;
  } = $props();

  const uid = $props.id();
  let hover = $state(-1);
  let width = $state(0);
  let readoutWidth = $state(0);
  let columns: HTMLElement | undefined = $state();

  let count = $derived(values.length);
  let cursor = $derived(hover >= 0 ? hover : live);
  let peak = $derived(Math.max(average ?? 0, ...values, 1));
  let scale = $derived(timeScale(peak));
  let lines = $derived(axis ? gridLines(scale) : []);
  const percent = (value: number) => Math.min(100, Math.max(0, (value / scale.top) * 100));
  // A bar with any time stands at least a few pixels tall, so a minute reads
  // as a minute rather than as nothing.
  const shown = (value: number) => (value > 0 ? Math.max(percent(value), (3 / height) * 100) : 0);

  let readoutLeft = $derived.by(() => {
    if (hover < 0 || count === 0) return 0;
    const center = ((hover + 0.5) / count) * width;
    const half = readoutWidth / 2;
    return Math.min(Math.max(center, half), Math.max(half, width - half));
  });

  let summary = $derived(
    values
      .map((value, index) => (value > 0 ? `${describe(index)}: ${format(value)}` : null))
      .filter((entry) => entry !== null)
      .join(", "),
  );

  function point(event: PointerEvent) {
    if (!columns) return;
    const box = columns.getBoundingClientRect();
    const x = document.dir === "rtl" ? box.right - event.clientX : event.clientX - box.left;
    hover = bucketAt(x, box.width, count);
  }

  function key(event: KeyboardEvent) {
    const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
    if (step !== undefined) {
      event.preventDefault();
      const from = hover >= 0 ? hover : live >= 0 ? live : count - 1;
      hover = Math.min(count - 1, Math.max(0, from + (document.dir === "rtl" ? -step : step)));
    } else if ((event.key === "Enter" || event.key === " ") && hover >= 0 && onselect) {
      event.preventDefault();
      onselect(hover);
    } else if (event.key === "Escape") {
      hover = -1;
    }
  }
</script>

<figure class="bars" class:axis style:--plot-height={`${height}px`}>
  <!-- Arrow keys walk the buckets, so it is announced as a slider whose
       value is the bucket in the readout. -->
  <div
    class="plot"
    role="slider"
    aria-label={label}
    aria-describedby={summary ? `${uid}-summary` : undefined}
    aria-valuemin={0}
    aria-valuemax={Math.max(0, count - 1)}
    aria-valuenow={Math.max(0, cursor)}
    aria-valuetext={cursor >= 0 ? `${describe(cursor)}: ${format(values[cursor] ?? 0)}` : label}
    tabindex="0"
    class:selectable={!!onselect}
    onpointermove={point}
    onpointerleave={() => (hover = -1)}
    onblur={() => (hover = -1)}
    onkeydown={key}
    onclick={() => {
      if (hover >= 0) onselect?.(hover);
    }}
  >
    {#each lines as line (line)}
      <span class="rule" style:bottom={`${percent(line)}%`}
        ><span class="value">{format(line)}</span></span
      >
    {/each}
    {#if average !== null && average > 0}
      <span class="average" style:bottom={`${percent(average)}%`}></span>
    {/if}
    <div class="columns" bind:this={columns} bind:clientWidth={width}>
      {#each values as value, index (index)}
        <span
          class="column"
          class:live={index === live}
          class:active={index === hover}
          class:selected={index === selected}
          class:dimmed={highlight !== null}
          style:--rise={`${index * 14}ms`}
        >
          <span class="track">
            {#if value > 0}
              <span class="bar" style:transform={`translateY(${100 - shown(value)}%)`}></span>
            {:else}
              <span class="stub"></span>
            {/if}
            {#if highlight !== null && (highlight[index] ?? 0) > 0}
              <span
                class="bar part"
                style:transform={`translateY(${100 - shown(highlight[index] ?? 0)}%)`}
              ></span>
            {/if}
          </span>
        </span>
      {/each}
    </div>
    {#if hover >= 0}
      <span
        class="readout"
        aria-hidden="true"
        bind:clientWidth={readoutWidth}
        style:inset-inline-start={`${readoutLeft}px`}
        ><span class="when">{describe(hover)}</span><span class="amount"
          >{format(highlight?.[hover] ?? values[hover] ?? 0)}</span
        ></span
      >
    {/if}
  </div>
  {#if summary}<span id={`${uid}-summary`} class="summary">{summary}</span>{/if}
  {#if ticks.length > 0}
    <div class="ticks" aria-hidden="true">
      {#each ticks as tick (tick.index)}
        <!-- A label under the first or last bar keeps inside the chart rather
             than centring past its edge. -->
        <span
          class:current={tick.index === live || tick.index === selected}
          class:first={tick.index === 0 && count > 7}
          style:inset-inline-start={tick.index === 0 && count > 7
            ? "0px"
            : `${((tick.index + 0.5) / Math.max(1, count)) * 100}%`}>{tick.label}</span
        >
      {/each}
    </div>
  {/if}
</figure>

<style>
  .bars {
    --bar-rest: color-mix(in srgb, var(--color-text) 26%, transparent);
    --bar-live: color-mix(in srgb, var(--color-text) 62%, transparent);
    --bar-hot: var(--color-text);
    --bar-dim: color-mix(in srgb, var(--color-text) 11%, transparent);
    --axis-room: 0px;

    margin: 0;
    min-width: 0;
  }

  .bars.axis {
    --axis-room: 40px;
  }

  .plot {
    position: relative;
    block-size: var(--plot-height);
    padding-inline-end: var(--axis-room);
    border-radius: var(--radius-inset);
    outline: none;
    touch-action: pan-y;
  }

  .plot.selectable {
    cursor: pointer;
  }

  .plot:focus-visible {
    box-shadow: 0 0 0 2px var(--color-ring);
  }

  .rule {
    position: absolute;
    inset-inline: 0 var(--axis-room);
    block-size: 0;
    border-block-start: 1px solid var(--color-border);
    pointer-events: none;
  }

  .value {
    position: absolute;
    inset-inline-start: calc(100% + 8px);
    inset-block-start: -7px;
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
    line-height: 14px;
    white-space: nowrap;
  }

  .average {
    position: absolute;
    inset-inline: 0 var(--axis-room);
    block-size: 0;
    border-block-start: 1px dashed color-mix(in srgb, var(--color-text) 34%, transparent);
    pointer-events: none;
    z-index: 1;
  }

  .columns {
    position: absolute;
    inset: 0 var(--axis-room) 0 0;
    display: flex;
    align-items: stretch;
    gap: clamp(2px, 1.1%, 6px);
    border-block-end: 1px solid var(--color-border-strong);
  }

  :global([dir="rtl"]) .columns {
    inset: 0 0 0 var(--axis-room);
  }

  .column {
    position: relative;
    flex: 1;
    min-width: 0;
  }

  .track {
    position: absolute;
    inset: 0;
    overflow: hidden;
    border-start-start-radius: 3px;
    border-start-end-radius: 3px;
  }

  .bar {
    position: absolute;
    inset: 0;
    border-start-start-radius: 3px;
    border-start-end-radius: 3px;
    background: var(--bar-rest);
    transition:
      transform var(--motion-page) var(--ease-emphasized),
      background-color var(--motion-fast) var(--ease-out);
    animation: rise var(--motion-page) var(--ease-emphasized) var(--rise) backwards;
  }

  .stub {
    position: absolute;
    inset-inline: 0;
    inset-block-end: 0;
    block-size: 2px;
    border-radius: var(--radius-capsule);
    background: var(--bar-dim);
  }

  .live .bar {
    background: var(--bar-live);
  }

  .selected .bar,
  .active .bar {
    background: var(--bar-hot);
  }

  .dimmed .bar:not(.part) {
    background: var(--bar-dim);
  }

  .bar.part {
    background: var(--bar-live);
  }

  .active .bar.part,
  .selected .bar.part {
    background: var(--bar-hot);
  }

  @keyframes rise {
    from {
      transform: translateY(100%);
    }
  }

  .readout {
    position: absolute;
    inset-block-end: calc(100% + 6px);
    display: inline-flex;
    gap: 6px;
    align-items: baseline;
    padding: 3px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-float);
    box-shadow: var(--shadow-float);
    font-size: var(--text-caption);
    line-height: 16px;
    white-space: nowrap;
    translate: -50% 0;
    pointer-events: none;
    z-index: 2;
  }

  :global([dir="rtl"]) .readout {
    translate: 50% 0;
  }

  .when {
    color: var(--color-muted);
  }

  .amount {
    color: var(--color-text);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .summary {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  .ticks {
    position: relative;
    block-size: 18px;
    margin-inline-end: var(--axis-room);
    margin-block-start: 6px;
  }

  .ticks span {
    position: absolute;
    translate: -50% 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 14px;
    white-space: nowrap;
  }

  :global([dir="rtl"]) .ticks span {
    translate: 50% 0;
  }

  .ticks span.first {
    translate: 0 0;
  }

  .ticks span.current {
    color: var(--color-text);
    font-weight: 600;
  }

  @media (prefers-reduced-motion: reduce) {
    .bar {
      animation: none;
      transition: none;
    }
  }

  :global(:root[data-reduce-motion="true"]) .bar {
    animation: none;
    transition: none;
  }

  @media (forced-colors: active) {
    .bar {
      background: CanvasText;
    }

    .dimmed .bar:not(.part) {
      background: GrayText;
    }
  }
</style>
