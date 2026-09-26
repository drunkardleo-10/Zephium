# Charts

One kit draws every chart in the product: Work's results and the Activity
views. It lives in `frame/src/shared/ui/data/Chart`, takes one spec type, and
draws SVG in Svelte on `d3-scale`, `d3-shape`, `d3-array` and `d3-time`. No
chart library, no `d3-selection`, no d3 transitions.

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
  compact?: boolean; // the card: no axis labels, no legend, no values table
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

| Kind | Use it for | Notes |
| --- | --- | --- |
| `bars` | Comparing a few things: prices, scores, time per site | Lie down past eight categories or with long names (never on a card). Several series group side by side with a legend. Values sit on the bars when there is room. |
| `stacked` | One total per x made of parts: a day's time by category | Diverging stacks for negatives; a 2px surface gap parts segments. |
| `line` | A trend over ordered x | Monotone curve, its own y range (zero not forced), points on hover, gaps at null. |
| `area` | A trend whose volume matters | As `line`, filled to zero with a 10% wash. |
| `range` | A spread per item: "$3,000–8,000" | A bar from `y` to `y2`, both ends rounded. |
| `donut` | A share of one whole | First series only, largest first, seven slices then "Other"; the total in the middle. |
| `heat` | Two categorical axes and a magnitude: hour by day | x categories across, series names down, one tone from the surface rung to the lit rung. |
| `spark` | A trend in a row | No axes, no tooltip, no table; a line, or bars when `x.kind` is `category`. Fills its container; pass `height` if it has none. |

## Tokens

- Series identity: `--chart-1` … `--chart-8`, in fixed order, never cycled; a
  ninth series takes `--color-tint-graphite`. Both themes are stepped and
  validated for colour-vision separation against their own surface.
- A single series (bars, lines, areas, a spark) and a donut's first slice are
  `--chart-1`. `--color-lit` is a light tonal rung in the dark theme, so a
  series drawn with it reads white on a dark card; the lit rung stays for what
  is on: the hover and the highlighted range. Heat mixes `--color-surface`
  toward `--color-lit`.
- Gridlines are hairlines on `--color-border` (zero on `--color-border-strong`);
  there are no axis lines. Ticks are rounded (`axisTicks`, clock steps for
  durations) in `--text-caption` and `--color-faint`.
- The tooltip is a plain popover on `--color-float` with `--shadow-float` and
  `--radius-control-compact`: the point, its value, and its sources as chips.
- No gradients, no glow, no blur. A bar's data end is rounded in proportion to
  its thickness; its baseline is square.

## Motion

A chart arrives once: bars grow from their baseline and lines draw
(`pathLength`) over `--motion-slow` on `--ease-emphasized`; washes, slices and
cells fade in. Hover answers on `--motion-fast`. Under reduced motion (system
or in-app) nothing animates; the chart is drawn settled.

## Accessibility

The plot is `role="img"`, named by the title, the axis labels, the kind and the
extremes ("lowest Mon at 5, highest Wed at 30"). In full size a `<details>`
values table carries every point, its exact words and its sources.

## Callers

**Work.** `Artifact` lazy-loads `WorkChart.svelte`, which maps the runtime's
chart (exact decimal strings) onto the spec once: `bars` by default, `range`
when any value parses as a range (`3000-8000`, `$3,000–8,000`), `line` past 24
points; a currency symbol makes it money; an unparseable value is a gap; the
original string stays in `display` for the table, under the precision note. The
card is `compact` and draws no basis and no legend.

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
