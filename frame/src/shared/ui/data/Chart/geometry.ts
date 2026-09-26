import { max, min } from "d3-array";
import { scaleBand, scaleLinear, scalePoint } from "d3-scale";
import { arc, area, curveMonotoneX, line, pie, stack, stackOffsetDiverging } from "d3-shape";
import { timeTicks } from "d3-time";
import {
  axisTicks,
  durationTicks,
  parseTime,
  pointEvidence,
  pointText,
  seriesColor,
  tickLabel,
  xText,
  type ChartEvidence,
  type ChartPoint,
  type ChartSpec,
} from "./chart";

export type Anchor = { x: number; y: number };
type Tick = { pos: number; label: string; zero: boolean };
type Label = { pos: number; text: string };
type Hit = {
  index: number;
  x: number;
  y: number;
  width: number;
  height: number;
  anchor: Anchor;
};
export type Bar = {
  index: number;
  series: number;
  d: string;
  color: string;
  /** Where the bar grows from: its baseline. */
  origin: Anchor;
  /** Its data end, where a tooltip points. */
  tip: Anchor;
  axis: "x" | "y";
  value?: { x: number; y: number; text: string; anchor: "start" | "middle" | "end" };
};
export type Trace = {
  series: number;
  color: string;
  line: string;
  area?: string;
  dots: { index: number; x: number; y: number }[];
};
export type Slice = {
  index: number;
  d: string;
  color: string;
  label: string;
  value: number;
  anchor: Anchor;
};
export type Cell = {
  index: number;
  x: number;
  y: number;
  width: number;
  height: number;
  /** 0 is the surface rung, 1 the lit rung; null is no value. */
  tone: number | null;
};
export type Geometry = {
  width: number;
  height: number;
  plot: { left: number; top: number; right: number; bottom: number };
  /** Category keys in drawing order; a hover index points into this (or slices, or cells). */
  keys: string[];
  ticks: Tick[];
  valueAxis: "x" | "y" | null;
  labels: Label[];
  rows: Label[];
  bars: Bar[];
  traces: Trace[];
  slices: Slice[];
  cells: Cell[];
  hits: Hit[];
  cursor: boolean;
  /** A donut's middle and the total it states there. */
  centre?: Anchor;
  total?: number;
};
export type Readout = {
  title: string;
  rows: { name: string; color: string; text: string }[];
  evidence: ChartEvidence[];
};

const CHAR = 6.5;
const width = (text: string) => text.length * CHAR;
const DONUT_SLICES = 8;

/** Categories in first-seen order; a time or number axis reads in its own order. */
export function categoryKeys(spec: ChartSpec): string[] {
  const keys: string[] = [];
  const seen = new Set<string>();
  for (const series of spec.series)
    for (const point of series.points) {
      const key = String(point.x);
      if (!seen.has(key)) {
        seen.add(key);
        keys.push(key);
      }
    }
  const kind = spec.x?.kind;
  if (kind === "time") keys.sort((a, b) => parseTime(a) - parseTime(b));
  if (kind === "linear") keys.sort((a, b) => Number(a) - Number(b));
  return keys;
}

function table(spec: ChartSpec, keys: readonly string[]): (ChartPoint | undefined)[][] {
  return spec.series.map((series) => {
    const byKey = new Map(series.points.map((point) => [String(point.x), point]));
    return keys.map((key) => byKey.get(key));
  });
}

/** Long names or many of them lie down, so every label stays readable. */
export function horizontal(spec: ChartSpec, keys: readonly string[]): boolean {
  if (spec.kind !== "bars" && spec.kind !== "range") return false;
  const long = keys.some((key) => xText(key, spec.x?.kind).length > 14);
  return spec.compact ? long && keys.length <= 6 : long || keys.length > 8;
}

function valueTicks(spec: ChartSpec, values: number[], count: number, zero: boolean): number[] {
  const low = Math.min(spec.y?.min ?? Infinity, min(values) ?? 0);
  const high = Math.max(spec.y?.max ?? -Infinity, max(values) ?? 0);
  return spec.y?.format === "duration"
    ? durationTicks(low, high, count)
    : axisTicks(low, high, count, zero);
}

/** A bar with its data end rounded and its baseline square. */
export function barPath(
  x: number,
  y: number,
  w: number,
  h: number,
  end: "top" | "bottom" | "left" | "right" | "both" | "none",
): string {
  const r = Math.max(
    0,
    Math.min(
      (end === "top" || end === "bottom" ? w : h) / 6,
      (end === "top" || end === "bottom" ? h : w) / 2,
      4,
    ),
  );
  if (end === "none" || r === 0) return `M${x},${y}h${w}v${h}h${-w}Z`;
  if (end === "both")
    return w > h
      ? `M${x + r},${y}h${w - 2 * r}a${r},${r} 0 0 1 ${r},${r}v${h - 2 * r}a${r},${r} 0 0 1 ${-r},${r}h${-(w - 2 * r)}a${r},${r} 0 0 1 ${-r},${-r}v${-(h - 2 * r)}a${r},${r} 0 0 1 ${r},${-r}Z`
      : `M${x},${y + r}a${r},${r} 0 0 1 ${r},${-r}h${w - 2 * r}a${r},${r} 0 0 1 ${r},${r}v${h - 2 * r}a${r},${r} 0 0 1 ${-r},${r}h${-(w - 2 * r)}a${r},${r} 0 0 1 ${-r},${-r}Z`;
  if (end === "top")
    return `M${x},${y + h}v${-(h - r)}a${r},${r} 0 0 1 ${r},${-r}h${w - 2 * r}a${r},${r} 0 0 1 ${r},${r}v${h - r}Z`;
  if (end === "bottom")
    return `M${x},${y}h${w}v${h - r}a${r},${r} 0 0 1 ${-r},${r}h${-(w - 2 * r)}a${r},${r} 0 0 1 ${-r},${-r}Z`;
  if (end === "right")
    return `M${x},${y}h${w - r}a${r},${r} 0 0 1 ${r},${r}v${h - 2 * r}a${r},${r} 0 0 1 ${-r},${r}h${-(w - r)}Z`;
  return `M${x + w},${y}v${h}h${-(w - r)}a${r},${r} 0 0 1 ${-r},${-r}v${-(h - 2 * r)}a${r},${r} 0 0 1 ${r},${-r}Z`;
}

function empty(width: number, height: number): Geometry {
  return {
    width,
    height,
    plot: { left: 0, top: 0, right: width, bottom: height },
    keys: [],
    ticks: [],
    valueAxis: null,
    labels: [],
    rows: [],
    bars: [],
    traces: [],
    slices: [],
    cells: [],
    hits: [],
    cursor: false,
  };
}

/** Everything a mark draws, in the chart's own pixels, for a container this wide. */
export function geometry(
  spec: ChartSpec,
  board: number,
  { other = "Other", sparkHeight = 24 }: { other?: string; sparkHeight?: number } = {},
): Geometry {
  const keys = categoryKeys(spec);
  switch (spec.kind) {
    case "donut":
      return donut(spec, board, other);
    case "heat":
      return heat(spec, keys, board);
    case "spark":
      return spark(spec, keys, board, sparkHeight);
    case "line":
    case "area":
      return traces(spec, keys, board);
    default:
      return horizontal(spec, keys) ? lying(spec, keys, board) : standing(spec, keys, board);
  }
}

/** Only as many labels as fit; the values table keeps every one of them. */
function thin(labels: Label[], room: number): Label[] {
  const longest = Math.max(1, ...labels.map((label) => width(label.text) + 12));
  const step = Math.max(1, Math.ceil(labels.length / Math.max(1, Math.floor(room / longest))));
  return labels.filter((_, index) => index % step === 0);
}

function leftPad(spec: ChartSpec, ticks: number[]): number {
  return spec.compact
    ? 4
    : Math.max(24, ...ticks.map((tick) => width(tickLabel(tick, spec.y)) + 10));
}

function standing(spec: ChartSpec, keys: string[], board: number): Geometry {
  const compact = !!spec.compact;
  const height = compact ? 132 : 232;
  const cells = table(spec, keys);
  const stacked = spec.kind === "stacked";
  const values = stacked
    ? stackedExtent(cells)
    : cells
        .flat()
        .flatMap((point) => (point && point.y !== null ? [point.y, point.y2 ?? point.y] : []));
  const ticks = valueTicks(spec, values, compact ? 3 : 5, true);
  const plot = {
    left: leftPad(spec, ticks),
    top: compact ? 8 : 18,
    right: board - (compact ? 4 : 8),
    bottom: height - (compact ? 4 : 24),
  };
  const y = scaleLinear()
    .domain([ticks[0] ?? 0, ticks.at(-1) ?? 1])
    .range([plot.bottom, plot.top]);
  const band = scaleBand()
    .domain(keys)
    .range([plot.left, plot.right])
    .paddingInner(0.3)
    .paddingOuter(0.15);
  const base = y(Math.max(ticks[0] ?? 0, Math.min(0, ticks.at(-1) ?? 0)));
  const bars: Bar[] = [];
  const count = spec.series.length;
  const sub = scaleBand<number>()
    .domain(stacked ? [0] : spec.series.map((_, order) => order))
    .range([0, band.bandwidth()])
    .paddingInner(0.12);
  const thickness = Math.max(2, Math.min(compact ? 18 : 24, sub.bandwidth()));
  const single = count === 1;
  if (stacked) {
    for (const layer of stackLayers(cells, keys))
      for (const [index, [from, to]] of layer.points.entries()) {
        if (from === to) continue;
        const x = band(keys[index]!)! + (band.bandwidth() - thickness) / 2;
        const up = to > from;
        const near = y(from);
        const far = y(to);
        // A 2px surface gap parts a segment from the one before it, never from the baseline.
        const gap = from !== 0 && Math.abs(near - far) > 3 ? 2 : 0;
        const end = layer.outer[index] ? (up ? "top" : "bottom") : "none";
        bars.push({
          index,
          series: layer.series,
          d: up
            ? barPath(x, far, thickness, near - far - gap, end)
            : barPath(x, near + gap, thickness, far - near - gap, end),
          color: seriesColor(layer.series),
          origin: { x: x + thickness / 2, y: base },
          tip: { x: x + thickness / 2, y: far },
          axis: "y",
        });
      }
  } else
    for (const [order, row] of cells.entries())
      for (const [index, point] of row.entries()) {
        if (!point || point.y === null) continue;
        const slot = band(keys[index]!)! + cluster(band.bandwidth(), thickness, count, order);
        const ranged = spec.kind === "range" && point.y2 !== undefined;
        const lowValue = ranged ? point.y : Math.min(0, point.y);
        const highValue = ranged ? point.y2! : Math.max(0, point.y);
        const top = y(highValue);
        const size = Math.max(1, y(lowValue) - top);
        const up = ranged || point.y >= 0;
        const text = pointText(point, spec.y);
        const room = single && !compact && width(text) + 4 <= band.step();
        bars.push({
          index,
          series: order,
          d: barPath(slot, top, thickness, size, ranged ? "both" : up ? "top" : "bottom"),
          color: seriesColor(order),
          origin: { x: slot + thickness / 2, y: ranged ? top + size : base },
          tip: { x: slot + thickness / 2, y: up ? top : top + size },
          axis: "y",
          ...(room
            ? {
                value: {
                  x: slot + thickness / 2,
                  y: up ? top - 6 : top + size + 12,
                  text,
                  anchor: "middle" as const,
                },
              }
            : {}),
        });
      }
  const labels = compact
    ? []
    : thin(
        keys.map((key) => ({
          pos: band(key)! + band.bandwidth() / 2,
          text: xText(key, spec.x?.kind),
        })),
        plot.right - plot.left,
      );
  return {
    ...empty(board, height),
    plot,
    keys,
    ticks: ticks.map((tick) => ({
      pos: y(tick),
      label: compact ? "" : tickLabel(tick, spec.y),
      zero: tick === 0,
    })),
    valueAxis: "y",
    labels,
    bars,
    hits: keys.map((key, index) => {
      // Above the value label when one is drawn, so the popover never covers it.
      const tip =
        min(
          bars
            .filter((bar) => bar.index === index)
            .map((bar) => Math.min(bar.tip.y, (bar.value?.y ?? Infinity) - 10)),
        ) ?? base;
      return {
        index,
        x: band(key)! - (band.step() - band.bandwidth()) / 2,
        y: plot.top,
        width: band.step(),
        height: plot.bottom - plot.top,
        anchor: { x: band(key)! + band.bandwidth() / 2, y: Math.min(tip, base) },
      };
    }),
  };
}

/** Bars of one category sit together, parted by the 2px surface gap, centred in the band. */
function cluster(room: number, thickness: number, count: number, order: number): number {
  const span = count * thickness + (count - 1) * 2;
  return (room - span) / 2 + order * (thickness + 2);
}

function stackedExtent(cells: (ChartPoint | undefined)[][]): number[] {
  const columns = cells[0]?.length ?? 0;
  const out: number[] = [];
  for (let index = 0; index < columns; index++) {
    let up = 0;
    let down = 0;
    for (const row of cells) {
      const value = row[index]?.y ?? 0;
      if (value > 0) up += value;
      else down += value;
    }
    out.push(up, down);
  }
  return out;
}

function stackLayers(cells: (ChartPoint | undefined)[][], keys: string[]) {
  const rows = keys.map((_, index) => cells.map((row) => row[index]?.y ?? 0));
  const layers = stack<number[], number>()
    .keys(cells.map((_, order) => order))
    .value((row, order) => row[order] ?? 0)
    .offset(stackOffsetDiverging)(rows);
  // The outermost segment on each side of the baseline carries the rounded data end.
  const outer = layers.map(() => keys.map(() => false));
  keys.forEach((_, index) => {
    let up = -1;
    let down = -1;
    layers.forEach((layer, order) => {
      const [from, to] = layer[index]!;
      if (from === to) return;
      if (from >= 0) up = order;
      else down = order;
    });
    if (up >= 0) outer[up]![index] = true;
    if (down >= 0) outer[down]![index] = true;
  });
  return layers.map((layer, order) => ({
    series: order,
    // Positive segments run baseline-out as [from, to]; negative ones are stored low-first.
    points: layer.map((segment) =>
      segment[0] >= 0 ? ([segment[0], segment[1]] as const) : ([segment[1], segment[0]] as const),
    ),
    outer: outer[order]!,
  }));
}

function lying(spec: ChartSpec, keys: string[], board: number): Geometry {
  const compact = !!spec.compact;
  const cells = table(spec, keys);
  const count = spec.series.length;
  const row = compact ? 20 : Math.max(22, 10 + count * 12);
  const values = cells
    .flat()
    .flatMap((point) => (point && point.y !== null ? [point.y, point.y2 ?? point.y] : []));
  const ticks = valueTicks(spec, values, compact ? 3 : 4, true);
  const texts = cells.map((series) =>
    series.map((point) => (point ? pointText(point, spec.y) : "")),
  );
  const showValues = count === 1 && !compact;
  const names = keys.map((key) => clip(xText(key, spec.x?.kind), compact ? 18 : 28));
  const top = compact ? 4 : 8;
  const bottomPad = compact ? 4 : 22;
  const height = top + keys.length * row + bottomPad;
  const plot = {
    left: Math.min(board * 0.42, Math.max(...names.map(width)) + 12),
    top,
    right: board - (showValues ? Math.max(...texts.flat().map(width)) + 10 : 8),
    bottom: height - bottomPad,
  };
  const x = scaleLinear()
    .domain([ticks[0] ?? 0, ticks.at(-1) ?? 1])
    .range([plot.left, plot.right]);
  const band = scaleBand()
    .domain(keys)
    .range([plot.top, plot.bottom])
    .paddingInner(0.3)
    .paddingOuter(0.1);
  const sub = scaleBand<number>()
    .domain(spec.series.map((_, order) => order))
    .range([0, band.bandwidth()])
    .paddingInner(0.12);
  const thickness = Math.max(2, Math.min(compact ? 14 : 18, sub.bandwidth()));
  const base = x(Math.max(ticks[0] ?? 0, Math.min(0, ticks.at(-1) ?? 0)));
  const bars: Bar[] = [];
  for (const [order, series] of cells.entries())
    for (const [index, point] of series.entries()) {
      if (!point || point.y === null) continue;
      const slot = band(keys[index]!)! + cluster(band.bandwidth(), thickness, count, order);
      const ranged = spec.kind === "range" && point.y2 !== undefined;
      const low = ranged ? point.y : Math.min(0, point.y);
      const high = ranged ? point.y2! : Math.max(0, point.y);
      const left = x(low);
      const size = Math.max(1, x(high) - left);
      const right = ranged || point.y >= 0;
      bars.push({
        index,
        series: order,
        d: barPath(left, slot, size, thickness, ranged ? "both" : right ? "right" : "left"),
        color: seriesColor(order),
        origin: { x: ranged ? left : base, y: slot + thickness / 2 },
        tip: { x: right ? left + size : left, y: slot },
        axis: "x",
        ...(showValues
          ? {
              value: {
                x: right ? left + size + 6 : left - 6,
                y: slot + thickness / 2 + 3.5,
                text: texts[order]![index]!,
                anchor: right ? ("start" as const) : ("end" as const),
              },
            }
          : {}),
      });
    }
  return {
    ...empty(board, height),
    plot,
    keys,
    ticks: compact
      ? ticks.map((tick) => ({ pos: x(tick), label: "", zero: tick === 0 }))
      : ticks.map((tick) => ({ pos: x(tick), label: tickLabel(tick, spec.y), zero: tick === 0 })),
    valueAxis: "x",
    rows: keys.map((key, index) => ({
      pos: band(key)! + band.bandwidth() / 2,
      text: names[index]!,
    })),
    bars,
    hits: keys.map((key, index) => ({
      index,
      x: 0,
      y: band(key)! - (band.step() - band.bandwidth()) / 2,
      width: board,
      height: band.step(),
      anchor: {
        x: max(bars.filter((bar) => bar.index === index).map((bar) => bar.tip.x)) ?? base,
        y: band(key)!,
      },
    })),
  };
}

function clip(text: string, length: number): string {
  return text.length > length ? `${text.slice(0, length - 1)}…` : text;
}

function position(spec: ChartSpec, keys: string[], range: [number, number]) {
  const kind = spec.x?.kind;
  if (kind === "time" || kind === "linear") {
    const numbers = keys.map((key) => (kind === "time" ? parseTime(key) : Number(key)));
    const scale = scaleLinear()
      .domain([min(numbers) ?? 0, max(numbers) ?? 1])
      .range(range);
    return { at: (index: number) => scale(numbers[index]!), scale, numbers };
  }
  const scale = scalePoint().domain(keys).range(range).padding(0.5);
  return { at: (index: number) => scale(keys[index]!)!, scale: null, numbers: null };
}

function traces(spec: ChartSpec, keys: string[], board: number): Geometry {
  const compact = !!spec.compact;
  const height = compact ? 132 : 232;
  const cells = table(spec, keys);
  const values = cells.flat().flatMap((point) => (point && point.y !== null ? [point.y] : []));
  const filled = spec.kind === "area";
  const ticks = valueTicks(spec, values, compact ? 3 : 5, filled);
  const plot = {
    left: leftPad(spec, ticks),
    top: compact ? 8 : 18,
    right: board - (compact ? 6 : 12),
    bottom: height - (compact ? 6 : 24),
  };
  const y = scaleLinear()
    .domain([ticks[0] ?? 0, ticks.at(-1) ?? 1])
    .range([plot.bottom, plot.top]);
  const x = position(spec, keys, [plot.left, plot.right]);
  const base = y(Math.max(ticks[0] ?? 0, Math.min(0, ticks.at(-1) ?? 0)));
  const drawn = cells.map((row, order) => {
    const points = row.map((point, index) => ({ index, x: x.at(index), y: point?.y ?? null }));
    const defined = (point: { y: number | null }) => point.y !== null;
    const path = line<(typeof points)[number]>()
      .defined(defined)
      .x((point) => point.x)
      .y((point) => y(point.y!))
      .curve(curveMonotoneX)(points);
    const wash = filled
      ? area<(typeof points)[number]>()
          .defined(defined)
          .x((point) => point.x)
          .y0(base)
          .y1((point) => y(point.y!))
          .curve(curveMonotoneX)(points)
      : null;
    return {
      series: order,
      color: seriesColor(order),
      line: path ?? "",
      ...(wash ? { area: wash } : {}),
      dots: points
        .filter(defined)
        .map((point) => ({ index: point.index, x: point.x, y: y(point.y!) })),
    };
  });
  let labels: Label[] = [];
  if (!compact) {
    if (x.scale && spec.x?.kind === "time") {
      const [from, to] = x.scale.domain() as [number, number];
      const span = to - from;
      const format = new Intl.DateTimeFormat(
        undefined,
        span > 2 * 86_400_000
          ? { month: "short", day: "numeric" }
          : { hour: "numeric", minute: "2-digit" },
      );
      labels = timeTicks(
        new Date(from),
        new Date(to),
        Math.max(2, Math.floor((plot.right - plot.left) / 80)),
      ).map((tick) => ({ pos: x.scale(tick.getTime()), text: format.format(tick) }));
    } else if (x.scale)
      labels = x.scale
        .ticks(Math.max(2, Math.floor((plot.right - plot.left) / 64)))
        .map((tick) => ({
          pos: x.scale(tick),
          text: xText(tick, "linear"),
        }));
    else
      labels = thin(
        keys.map((key, index) => ({ pos: x.at(index), text: xText(key, spec.x?.kind) })),
        plot.right - plot.left,
      );
  }
  return {
    ...empty(board, height),
    plot,
    keys,
    ticks: ticks.map((tick) => ({
      pos: y(tick),
      label: compact ? "" : tickLabel(tick, spec.y),
      zero: tick === 0,
    })),
    valueAxis: "y",
    labels,
    traces: drawn,
    cursor: true,
    hits: keys.map((_, index) => {
      const here = x.at(index);
      const before = index > 0 ? (x.at(index - 1) + here) / 2 : plot.left;
      const after = index < keys.length - 1 ? (x.at(index + 1) + here) / 2 : plot.right;
      const tops = drawn.flatMap((trace) =>
        trace.dots.filter((dot) => dot.index === index).map((dot) => dot.y),
      );
      return {
        index,
        x: before,
        y: plot.top,
        width: Math.max(1, after - before),
        height: plot.bottom - plot.top,
        anchor: { x: here, y: min(tops) ?? plot.top },
      };
    }),
  };
}

type DonutPart = { label: string; value: number; points: ChartPoint[]; other?: boolean };

/** The slices a donut draws: the largest seven and "Other" once there are more than eight. */
export function donutParts(spec: ChartSpec, other: string): DonutPart[] {
  const parts: DonutPart[] = (spec.series[0]?.points ?? [])
    .filter((point) => point.y !== null && point.y > 0)
    .map((point) => ({ label: xText(point.x, spec.x?.kind), value: point.y!, points: [point] }))
    .sort((a, b) => b.value - a.value);
  if (parts.length <= DONUT_SLICES) return parts;
  const rest = parts.slice(DONUT_SLICES - 1);
  return [
    ...parts.slice(0, DONUT_SLICES - 1),
    {
      label: other,
      value: rest.reduce((sum, part) => sum + part.value, 0),
      points: rest.flatMap((part) => part.points),
      other: true,
    },
  ];
}

function donut(spec: ChartSpec, board: number, other: string): Geometry {
  const compact = !!spec.compact;
  const height = compact ? 132 : 200;
  const parts = donutParts(spec, other);
  const radius = Math.max(8, Math.min(board, height) / 2 - 4);
  const centre = { x: board / 2, y: height / 2 };
  const shape = arc<{ startAngle: number; endAngle: number; padAngle: number }>()
    .innerRadius(radius * 0.64)
    .outerRadius(radius);
  const angles = pie<DonutPart>()
    .sort(null)
    .value((part) => part.value)
    .padAngle(parts.length > 1 ? 0.012 : 0)(parts);
  return {
    ...empty(board, height),
    plot: {
      left: centre.x - radius,
      top: centre.y - radius,
      right: centre.x + radius,
      bottom: centre.y + radius,
    },
    keys: parts.map((part) => part.label),
    centre,
    total: parts.reduce((sum, part) => sum + part.value, 0),
    slices: angles.map((angle, index) => {
      const [x, y] = shape.centroid(angle);
      const part = parts[index]!;
      return {
        index,
        d: shape(angle) ?? "",
        color: part.other ? "var(--color-tint-graphite)" : seriesColor(index),
        label: part.label,
        value: part.value,
        anchor: { x: centre.x + x, y: centre.y + y },
      };
    }),
  };
}

function heat(spec: ChartSpec, keys: string[], board: number): Geometry {
  const compact = !!spec.compact;
  const cells = table(spec, keys);
  const rowsCount = spec.series.length;
  const names = spec.series.map((series) => clip(series.name, 14));
  const left = compact ? 0 : Math.max(0, ...names.map(width)) + 14;
  const top = compact ? 0 : 4;
  const bottomPad = compact ? 0 : 20;
  const row = compact ? Math.max(6, Math.floor(132 / Math.max(1, rowsCount))) : 20;
  const height = top + rowsCount * row + bottomPad;
  const plot = { left, top, right: board, bottom: height - bottomPad };
  const column = scaleBand().domain(keys).range([plot.left, plot.right]).paddingInner(0.08);
  const values = cells.flat().flatMap((point) => (point && point.y !== null ? [point.y] : []));
  const tone = scaleLinear()
    .domain([Math.min(0, min(values) ?? 0), max(values) || 1])
    .range([0, 1])
    .clamp(true);
  const gap = 2;
  const out: Cell[] = [];
  for (const [order, series] of cells.entries())
    for (const [index, point] of series.entries())
      out.push({
        index: order * keys.length + index,
        x: column(keys[index]!)!,
        y: plot.top + order * row,
        width: column.bandwidth(),
        height: row - gap,
        tone: point && point.y !== null ? tone(point.y) : null,
      });
  return {
    ...empty(board, height),
    plot,
    keys,
    labels: compact
      ? []
      : thin(
          keys.map((key) => ({
            pos: column(key)! + column.bandwidth() / 2,
            text: xText(key, spec.x?.kind),
          })),
          plot.right - plot.left,
        ),
    rows: compact
      ? []
      : names.map((text, order) => ({ pos: plot.top + order * row + (row - gap) / 2, text })),
    cells: out,
    hits: out.map((cell) => ({
      index: cell.index,
      x: cell.x,
      y: cell.y,
      width: cell.width,
      height: cell.height,
      anchor: { x: cell.x + cell.width / 2, y: cell.y },
    })),
  };
}

function spark(spec: ChartSpec, keys: string[], board: number, height: number): Geometry {
  const cells = table(spec, keys);
  const values = cells.flat().flatMap((point) => (point && point.y !== null ? [point.y] : []));
  const bars = spec.x?.kind === "category";
  const low = bars ? Math.min(0, min(values) ?? 0) : (min(values) ?? 0);
  const high = max(values) ?? 1;
  const plot = { left: 1, top: 2, right: board - 1, bottom: height - 2 };
  const y = scaleLinear()
    .domain(low === high ? [low - 1, high + 1] : [low, high])
    .range([plot.bottom, plot.top]);
  if (bars) {
    const band = scaleBand().domain(keys).range([plot.left, plot.right]).paddingInner(0.25);
    return {
      ...empty(board, height),
      plot,
      keys,
      bars: (cells[0] ?? []).flatMap((point, index) =>
        point && point.y !== null
          ? [
              {
                index,
                series: 0,
                d: barPath(
                  band(keys[index]!)!,
                  y(Math.max(0, point.y)),
                  band.bandwidth(),
                  Math.max(1, Math.abs(y(point.y) - y(0))),
                  "none",
                ),
                color: seriesColor(0),
                origin: { x: band(keys[index]!)! + band.bandwidth() / 2, y: y(0) },
                tip: { x: band(keys[index]!)! + band.bandwidth() / 2, y: y(point.y) },
                axis: "y" as const,
              },
            ]
          : [],
      ),
    };
  }
  const x = position(spec, keys, [plot.left, plot.right]);
  return {
    ...empty(board, height),
    plot,
    keys,
    traces: cells.map((row, order) => {
      const points = row.map((point, index) => ({ index, x: x.at(index), y: point?.y ?? null }));
      return {
        series: order,
        color: seriesColor(order),
        line:
          line<(typeof points)[number]>()
            .defined((point) => point.y !== null)
            .x((point) => point.x)
            .y((point) => y(point.y!))
            .curve(curveMonotoneX)(points) ?? "",
        dots: [],
      };
    }),
  };
}

/** What the tooltip says for one hover index. */
export function readout(
  spec: ChartSpec,
  shape: Geometry,
  index: number,
  other: string,
): Readout | null {
  if (spec.kind === "donut") {
    const part = donutParts(spec, other)[index];
    const slice = shape.slices[index];
    if (!part || !slice) return null;
    const share = shape.total ? part.value / shape.total : 0;
    return {
      title: part.label,
      rows: [
        {
          name: new Intl.NumberFormat(undefined, {
            style: "percent",
            maximumFractionDigits: 1,
          }).format(share),
          color: slice.color,
          text:
            part.points.length === 1
              ? pointText(part.points[0], spec.y)
              : pointText({ x: "", y: part.value }, spec.y),
        },
      ],
      evidence: pointEvidence(part.points),
    };
  }
  if (spec.kind === "heat") {
    const order = Math.floor(index / Math.max(1, shape.keys.length));
    const key = shape.keys[index % Math.max(1, shape.keys.length)];
    const series = spec.series[order];
    if (!series || key === undefined) return null;
    const point = series.points.find((entry) => String(entry.x) === key);
    return {
      title: `${xText(key, spec.x?.kind)} · ${series.name}`,
      rows: [{ name: "", color: "var(--color-lit)", text: point ? pointText(point, spec.y) : "—" }],
      evidence: pointEvidence([point]),
    };
  }
  const key = shape.keys[index];
  if (key === undefined) return null;
  const points = spec.series.map((series) =>
    series.points.find((point) => String(point.x) === key),
  );
  const single = spec.series.length === 1;
  return {
    title: xText(key, spec.x?.kind),
    rows: spec.series.map((series, order) => ({
      name: single ? "" : series.name,
      color: seriesColor(order),
      text: pointText(points[order], spec.y),
    })),
    evidence: pointEvidence(points),
  };
}
