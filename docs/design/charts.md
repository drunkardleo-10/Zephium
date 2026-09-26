# Charts

One kit draws every chart in the product: Work's results and the Activity
views. It lives in `frame/src/shared/ui/data/Chart`, takes one spec type, and
draws SVG with [LayerChart](https://layerchart.com) `2.5.0` (pinned, Svelte 5,
on d3).

## What is ours, what is the library's

| Ours | LayerChart's |
| --- | --- |
| `ChartSpec` and its eight kinds (`chart.ts`) | Scales, stacking and grouping |
| Value parsing, formats, ticks, extremes (`chart.ts`) | Marks: bars, splines, areas, arcs, rects, text |
| The spec as LayerChart rows and series, the card rules, the tooltip's words, the CSV (`layer.ts`) | Axes, gridlines, the hover highlight |
| Work's conversion (`Artifact/work-chart.ts`, `WorkChart.svelte`) | Tooltip placement and hit areas |
| Tokens, the legend, the basis line, the values table (`Chart.svelte`) | Arrival motion (tweened bars, drawn lines) |

`Chart.svelte` renders through LayerChart's `ChartCore` with its own children:
one `<svg>` per chart, no canvas, one size observer (LayerChart's, on the plot
box), nothing running at rest.

LayerChart is patched (`patches/layerchart@2.5.0.patch`). Its lazy imports of
Voronoi, Arc, Bar, Spline, pan and zoom, and brush import back into the chart
chunk, which stops the bundler merging common chunks and splits the browser's
startup into dozens of requests. Arc, Bar and Spline are imported statically as
SVG; voronoi, pan, zoom and brush are not bundled, so a tooltip `mode` of
`voronoi`, or `transform` or `brush` on a chart, does nothing. Re-check the
patch when LayerChart is upgraded.

## The spec

```ts
type ChartSpec = {
  kind: "bars" | "stacked" | "line" | "area" | "range" | "donut" | "heat" | "spark";
  series: { name: string; points: ChartPoint[] }[];
  x?: { label?: string; kind?: "category" | "time" | "linear" };
  y?: {
    label?: string;
    unit?: string;
    format?: "number" | "money" | "duration" | "percent" | "bytes";
    currency?: string; // ISO code, for money
    min?: number; // bounds the axis must include
    max?: number;
  };
  basis?: string; // one line under the plot
  knowledge?: boolean; // from what the agent knows; draws nothing, the basis line speaks
  compact?: boolean; // the card: see the card rules
};
type ChartPoint = {
  x: string | number; // a category, a number, or an ISO date for a time axis
  y: number | null; // null is a gap; nothing is drawn or invented
  y2?: number; // the top of a range
  display?: string; // the value as its source wrote it; shown verbatim in the table
  evidence?: { key: string; label: string; origin?: string; url?: string; file?: boolean }[];
};
```

Values are numbers. Durations are seconds (`5040` reads `1h 24m`), percentages
are points out of 100 (`42` reads `42%`), bytes use decimal units (`1.2 GB`),
money and numbers go through `Intl.NumberFormat`. Every figure is tabular.
A bare ISO date on a time axis is that day's local midnight.

Render it with `loadChart()` (lazy) and `<Chart {title} {spec} {onevidence} {glyph} />`.
`title` names the chart for assistive technology; `glyph` is an optional snippet
that draws a source's mark in the tooltip (Work passes its favicon glyph).
`parseValue` and `formatValue` are exported for owners that hold text.

## The kinds

| Kind | Use it for | LayerChart | Notes |
| --- | --- | --- | --- |
| `bars` | Comparing a few things: prices, scores, time per site | `Bars`, grouped by series | Lie down past eight categories or with long names (never on a card). One series writes its values on the bars, up to twelve. |
| `stacked` | One total per x made of parts | `Bars`, `stackDiverging` | Negatives stack below zero; a 2px gap parts segments; only the outer segment is rounded. |
| `line` | A trend over ordered x | `Spline`, monotone | Its own y range (zero not forced), points on hover, gaps at null. |
| `area` | A trend whose volume matters | `Area` + `Spline` | As `line`, filled to zero with a 10% wash. |
| `range` | A spread per item: "$3,000–8,000" | `Bars` from `y` to `y2` | Both ends rounded. |
| `donut` | A share of one whole | `Pie` + `Arc` | First series only, largest first, seven slices then "Other"; the total in the middle. |
| `heat` | Two categorical axes and a magnitude: hour by day | `Rect` on two band scales | x categories across, series names down, one tone from the surface rung to the lit rung. |
| `spark` | A trend in a row | `Spline`, or `Bars` when `x.kind` is `category` | No axes, no tooltip, no table. Fills its container; pass `height` if it has none. |

## The card

`compact` is the Work card (300×160). It is never mute:

- **Bars and ranges** write the category under each bar and the value on top,
  at `--text-caption`, abbreviated with the spec's format (`1.2K`, `$3.4K`,
  `45%`, `$3K–$8K`). Past twelve bars there is no room and the values go.
- **Stacks** write their categories.
- **Lines and areas** write the first and last x under the plot and the first
  series' last value at its end.
- **A donut** states its largest slice's share in the middle, with its label.
- **Legend**: none for one series; a two-row legend for two to four; past four,
  the values table instead.

No axis labels and no ticks on the card; gridlines stay.

## The lift

Full size: axes with rounded ticks (`axisTicks`, clock steps for durations) and
labels, gridlines, the legend when there are two series or more, a tooltip on
hover with every series at that x and its sources as chips, the extremes
sentence under the plot ("lowest Mon at 5, highest Wed at 30"), the basis line,
and the values table with **Copy values (CSV)**, in the sources' own words.

## Tokens

- Series identity: `--chart-1` … `--chart-8`, in fixed order, never cycled; a
  ninth series takes `--color-tint-graphite`. A single series is `--chart-1`.
- Gridlines and axes are hairlines on `--color-border` (zero on
  `--color-border-strong`); there are no axis lines. Labels are
  `--text-caption` in `--color-muted`.
- The tooltip is our content in LayerChart's positioned tooltip: `--color-float`,
  `--radius-control`, `--shadow-control`; the point, its values and its sources.
- Heat mixes `--color-surface` toward `--color-lit`. The hover column is
  `--color-fill`.
- No gradients, no glow, no blur. A bar's data end is rounded; its baseline is
  square.

## Motion

A chart arrives once over `--motion-base`: bars grow from their baseline
(LayerChart's tweened props) and lines draw in; washes, slices and cells fade
in. Nothing animates at rest. Under reduced motion (system or in-app) nothing
animates and the tooltip neither springs nor fades.

## Accessibility

The plot is `role="img"`, named by the title, the axis labels, the kind and the
extremes. The values table (`<details>`) carries every point, its exact words
and its sources: always in the lift, and on a card with more than four series.

## Cost

The kit is its own lazy chunk (`WorkChart` today, `Chart` once Activity loads
it through `loadChart`): about 350 KB minified, 103 KB gzipped, with
LayerChart. Svelte's transition runtime, which LayerChart's marks use, sits in
the shared runtime chunk (+2.3 KB in every graph).

## Callers

**Work.** `Artifact` lazy-loads `WorkChart.svelte`, which maps the runtime's
chart (exact decimal strings) onto the spec once: `bars` by default, `range`
when any value parses as a range (`3000-8000`, `$3,000–8,000`), `line` past 24
points; a currency symbol makes it money; an unparseable value is a gap; the
original string stays in `display` for the table, under the precision note. The
card is `compact` and draws no basis.

```ts
workChartSpec({ xLabel: "Builder", yLabel: "Quote", series, basis: "Basis: Written quotes" });
// → { kind: "range", y: { label: "Quote", format: "money", currency: "USD" }, … }
```

**Activity.** Build the spec from measured numbers and render it directly.

```svelte
<script lang="ts">
  import LazyView from "$shared/ui/LazyView";
  import { loadChart, type ChartSpec } from "$shared/ui/data/Chart";
  const spec: ChartSpec = {
    kind: "bars",
    x: { label: "Site" },
    y: { label: "Time", format: "duration" },
    basis: "Measured on this Mac, last 7 days",
    series: [{ name: "Time", points: sites.map((site) => ({ x: site.host, y: site.seconds })) }],
  };
</script>

<LazyView loader={loadChart} {...labels}>
  {#snippet children(Chart)}<Chart title="Time by site" {spec} />{/snippet}
</LazyView>
```

Days as a trend: `kind: "area"`, `x: { kind: "time" }`, points `{ x: "2026-09-01", y }`.
Hour by day: `kind: "heat"`, one series per day, `x` the hour. A row's
sparkline: `kind: "spark"` inside a sized box.
