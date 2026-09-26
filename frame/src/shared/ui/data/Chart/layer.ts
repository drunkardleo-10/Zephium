import {
  axisTicks,
  durationTicks,
  formatValue,
  parseTime,
  pointEvidence,
  pointText,
  seriesColor,
  xText,
  type ChartEvidence,
  type ChartPoint,
  type ChartSpec,
  type ChartY,
} from "./chart";

/** The spec as LayerChart reads it: one row per category, one accessor per series. */
export type Row = {
  index: number;
  key: string;
  /** What the x scale takes: the key on a band, a Date on a time axis, a number on a linear one. */
  x: string | number | Date;
  label: string;
  points: (ChartPoint | undefined)[];
};
type Value = number | null;
export type PlanSeries = {
  key: string;
  label: string;
  color: string;
  value: ((row: Row) => Value) | [(row: Row) => Value, (row: Row) => Value];
};
export type Part = {
  index: number;
  label: string;
  value: number;
  color: string;
  points: ChartPoint[];
};
export type Cell = {
  index: number;
  key: string;
  /** The series, as a band on y. */
  order: string;
  row: string;
  label: string;
  value: number | null;
  /** 0 is the surface rung, 1 the lit rung; null is no value. */
  tone: number | null;
  point: ChartPoint | undefined;
};
export type Plan = {
  keys: string[];
  rows: Row[];
  series: PlanSeries[];
  /** Bars lying down: categories on y, values on x. */
  horizontal: boolean;
  scale: "band" | "time" | "linear";
  layout: "group" | "stackDiverging" | "overlap";
  /** The value axis: round ticks, and the domain they span. */
  ticks: number[];
  domain: [number, number];
  parts: Part[];
  total: number;
  cells: Cell[];
};
export type Readout = {
  title: string;
  rows: { name: string; color: string; text: string }[];
  evidence: ChartEvidence[];
};
export type Legend = "none" | "rows" | "table";
export type Card = {
  /** Category labels under the bars (or beside them, lying down). */
  categories: boolean;
  /** Each bar's value at its end, abbreviated with the spec's format. */
  values: boolean;
  /** A trend's first and last x and its last value. */
  ends: { first: string; last: string; value: string } | null;
  /** A donut's largest slice and its share. */
  lead: { label: string; share: string } | null;
  legend: Legend;
};

const DONUT_SLICES = 8;
/** Past this many bars a card has no room for a value on each. */
const CARD_VALUES = 12;

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

/** Long names or many of them lie down, so every label stays readable. */
export function horizontal(spec: ChartSpec, keys: readonly string[]): boolean {
  if (spec.kind !== "bars" && spec.kind !== "range") return false;
  const long = keys.some((key) => xText(key, spec.x?.kind).length > 14);
  return spec.compact ? long && keys.length <= 6 : long || keys.length > 8;
}

/** The slices a donut draws: the largest seven and "Other" once there are more than eight. */
export function donutParts(spec: ChartSpec, other: string): Part[] {
  const parts = (spec.series[0]?.points ?? [])
    .filter((point) => point.y !== null && point.y > 0)
    .map((point) => ({ label: xText(point.x, spec.x?.kind), value: point.y!, points: [point] }))
    .sort((a, b) => b.value - a.value);
  const kept =
    parts.length <= DONUT_SLICES
      ? parts
      : [
          ...parts.slice(0, DONUT_SLICES - 1),
          {
            label: other,
            value: parts.slice(DONUT_SLICES - 1).reduce((sum, part) => sum + part.value, 0),
            points: parts.slice(DONUT_SLICES - 1).flatMap((part) => part.points),
            other: true,
          },
        ];
  return kept.map((part, index) => ({
    index,
    label: part.label,
    value: part.value,
    points: part.points,
    color: "other" in part ? "var(--color-tint-graphite)" : seriesColor(index),
  }));
}

function valueTicks(spec: ChartSpec, values: number[], zero: boolean): number[] {
  const count = spec.compact ? 3 : 5;
  const finite = values.filter(Number.isFinite);
  const low = Math.min(spec.y?.min ?? Infinity, ...finite, zero ? 0 : Infinity);
  const high = Math.max(spec.y?.max ?? -Infinity, ...finite, zero ? 0 : -Infinity);
  if (!Number.isFinite(low) || !Number.isFinite(high)) return [0, 1];
  return spec.y?.format === "duration"
    ? durationTicks(low, high, count)
    : axisTicks(low, high, count, zero);
}

function stackedExtent(rows: Row[]): number[] {
  return rows.flatMap((row) => {
    let up = 0;
    let down = 0;
    for (const point of row.points) {
      const value = point?.y ?? 0;
      if (value > 0) up += value;
      else down += value;
    }
    return [up, down];
  });
}

function rowX(key: string, kind: NonNullable<ChartSpec["x"]>["kind"]): Row["x"] {
  if (kind === "time") return new Date(parseTime(key));
  if (kind === "linear") return Number(key);
  return key;
}

/** Everything LayerChart needs to draw the spec, computed once per spec. */
export function plan(spec: ChartSpec, other = "Other"): Plan {
  const keys = categoryKeys(spec);
  const trend = spec.kind === "line" || spec.kind === "area" || spec.kind === "spark";
  const scale =
    trend && (spec.x?.kind === "time" || spec.x?.kind === "linear") ? spec.x.kind : "band";
  const rows: Row[] = keys.map((key, index) => ({
    index,
    key,
    x: scale === "band" ? key : rowX(key, spec.x?.kind),
    label: xText(key, spec.x?.kind),
    points: spec.series.map((series) => series.points.find((point) => String(point.x) === key)),
  }));
  const stacked = spec.kind === "stacked";
  const series: PlanSeries[] = spec.series.map((entry, order) => {
    const y = (row: Row) => row.points[order]?.y ?? (stacked ? 0 : null);
    return {
      key: `s${order}`,
      label: entry.name,
      color: seriesColor(order),
      value:
        spec.kind === "range"
          ? [y, (row: Row) => row.points[order]?.y2 ?? row.points[order]?.y ?? null]
          : y,
    };
  });
  const values = stacked
    ? stackedExtent(rows)
    : rows.flatMap((row) =>
        row.points.flatMap((point) =>
          point && point.y !== null ? [point.y, point.y2 ?? point.y] : [],
        ),
      );
  const zero = spec.kind !== "line" && spec.kind !== "spark";
  const ticks = valueTicks(spec, values, zero);
  const parts = spec.kind === "donut" ? donutParts(spec, other) : [];
  return {
    keys,
    rows,
    series,
    horizontal: horizontal(spec, keys),
    scale,
    layout: stacked ? "stackDiverging" : trend ? "overlap" : "group",
    ticks,
    domain: [ticks[0] ?? 0, ticks.at(-1) ?? 1],
    parts,
    total: parts.reduce((sum, part) => sum + part.value, 0),
    cells: spec.kind === "heat" ? heatCells(spec, rows) : [],
  };
}

function heatCells(spec: ChartSpec, rows: Row[]): Cell[] {
  const values = rows.flatMap((row) =>
    row.points.flatMap((point) => (point && point.y !== null ? [point.y] : [])),
  );
  const low = Math.min(0, ...values);
  const high = Math.max(...values, 0) || 1;
  return spec.series.flatMap((series, order) =>
    rows.map((row) => {
      const point = row.points[order];
      const value = point?.y ?? null;
      return {
        index: order * rows.length + row.index,
        key: row.key,
        order: String(order),
        row: series.name,
        label: row.label,
        value,
        tone: value === null ? null : Math.min(1, Math.max(0, (value - low) / (high - low || 1))),
        point,
      };
    }),
  );
}

/** A value as short as a card needs it: 1.2K, $3.4K, 45%. */
export function shortValue(point: ChartPoint | undefined, y?: ChartY): string {
  if (!point || point.y === null) return "";
  const low = short(point.y, y);
  return point.y2 === undefined ? low : `${low}–${short(point.y2, y)}`;
}

const compactNumber = new Intl.NumberFormat(undefined, {
  notation: "compact",
  maximumFractionDigits: 1,
});
function short(value: number, y?: ChartY): string {
  const plain = !y?.format || y.format === "number" || (y.format === "money" && !y.currency);
  return plain ? compactNumber.format(value) : formatValue(value, y, true);
}

/** One series names itself; two to four take a two-row legend; more are read from the table. */
export function legendRule(count: number, compact: boolean): Legend {
  if (count < 2) return "none";
  return compact && count > 4 ? "table" : "rows";
}

const shares = new Intl.NumberFormat(undefined, { style: "percent", maximumFractionDigits: 0 });

/** What a card writes beside its marks, so it is never mute. */
export function card(spec: ChartSpec, shape: Plan): Card {
  const legend =
    spec.kind === "donut" || spec.kind === "heat" || spec.kind === "spark"
      ? "none"
      : legendRule(spec.series.length, true);
  const bars = spec.kind === "bars" || spec.kind === "range" || spec.kind === "stacked";
  let ends: Card["ends"] = null;
  if ((spec.kind === "line" || spec.kind === "area") && shape.rows.length) {
    const last = shape.rows.findLast((row) => row.points[0] && row.points[0].y !== null);
    ends = {
      first: shape.rows[0]!.label,
      last: shape.rows.at(-1)!.label,
      value: shortValue(last?.points[0], spec.y),
    };
  }
  const top = shape.parts[0];
  return {
    categories: bars,
    values:
      (spec.kind === "bars" || spec.kind === "range") &&
      shape.rows.length * spec.series.length <= CARD_VALUES,
    ends,
    lead:
      spec.kind === "donut" && top && shape.total
        ? { label: top.label, share: shares.format(top.value / shape.total) }
        : null,
    legend,
  };
}

/** What the tooltip says for a row, a slice or a cell. */
export function readout(spec: ChartSpec, shape: Plan, index: number): Readout | null {
  if (spec.kind === "donut") {
    const part = shape.parts[index];
    if (!part) return null;
    const share = shape.total ? part.value / shape.total : 0;
    return {
      title: part.label,
      rows: [
        {
          name: new Intl.NumberFormat(undefined, {
            style: "percent",
            maximumFractionDigits: 1,
          }).format(share),
          color: part.color,
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
    const cell = shape.cells[index];
    if (!cell) return null;
    return {
      title: `${cell.label} · ${cell.row}`,
      rows: [
        {
          name: "",
          color: "var(--color-lit)",
          text: cell.point ? pointText(cell.point, spec.y) : "—",
        },
      ],
      evidence: pointEvidence([cell.point]),
    };
  }
  const row = shape.rows[index];
  if (!row) return null;
  const single = spec.series.length === 1;
  return {
    title: row.label,
    rows: spec.series.map((series, order) => ({
      name: single ? "" : series.name,
      color: seriesColor(order),
      text: pointText(row.points[order], spec.y),
    })),
    evidence: pointEvidence(row.points),
  };
}

function csvCell(text: string): string {
  return /[",\n\r]/u.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
}

/** Every point as CSV, in the source's own words where it has them. */
export function valuesCsv(spec: ChartSpec): string {
  const keys = categoryKeys(spec);
  const header = [
    spec.x?.label ?? "",
    ...spec.series.map((series) => series.name || spec.y?.label || ""),
  ];
  const lines = keys.map((key) => [
    xText(key, spec.x?.kind),
    ...spec.series.map((series) => {
      const point = series.points.find((entry) => String(entry.x) === key);
      return point && (point.display !== undefined || point.y !== null)
        ? pointText(point, spec.y)
        : "";
    }),
  ]);
  return [header, ...lines].map((line) => line.map(csvCell).join(",")).join("\n");
}
