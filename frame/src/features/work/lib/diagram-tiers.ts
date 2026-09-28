import type { CanvasPosition } from "./canvas-model";
import {
  DIAGRAM,
  PLATE,
  bandsOf,
  lanesOf,
  midpoint,
  plateHeight,
  plateWidth,
  primaryFlows,
  type DiagramFlow,
  type DiagramLayout,
  type DiagramShape,
} from "./diagram";

/** Room a tiered picture keeps: between parts of a column, around a channel, beside a name. */
const ROW = 20;
const TRACK = 8;
const AIR = 14;
const TWIN = 6;
/** Flows sharing a side of a part stand this far apart, clear of its corners. */
const PORT = 14;
const PORT_EDGE = 14;

type Flow = DiagramShape["flows"][number];

/** Parts ordered by where their neighbours stand, a few sweeps each way. */
function order(columns: string[][], flows: readonly Flow[]): string[][] {
  const out = columns.map((column) => [...column]);
  const place = () => {
    const at = new Map<string, { column: number; row: number }>();
    out.forEach((column, index) => column.forEach((id, row) => at.set(id, { column: index, row })));
    return at;
  };
  for (let sweep = 0; sweep < 6; sweep += 1) {
    const at = place();
    const forward = sweep % 2 === 0;
    const range = out.map((_, index) => index);
    for (const index of forward ? range : range.reverse()) {
      const column = out[index]!;
      const score = new Map(
        column.map((id, row) => {
          const near = flows.flatMap((flow) => {
            const other = flow.from === id ? flow.to : flow.to === id ? flow.from : null;
            const spot = other ? at.get(other) : undefined;
            if (!spot || spot.column === index) return [];
            return forward === spot.column < index ? [spot.row] : [];
          });
          return [id, near.length ? near.reduce((a, b) => a + b, 0) / near.length : row];
        }),
      );
      column.sort((a, b) => score.get(a)! - score.get(b)!);
    }
  }
  return out;
}

/**
 * A tiered diagram laid out by hand: each tier one column, never split, in the
 * order the diagram states them; parts ordered to cross little, set level with
 * what they talk to; each flow an orthogonal line on a track of its own in the
 * gap before its target, over the tiers between on a clear row, its name on
 * the last run into its target.
 */
export function tierLayout(shape: DiagramShape): DiagramLayout {
  const { width: W, height: H } = DIAGRAM.node;
  const band = DIAGRAM.layer;
  const lanes = shape.lanes.length + 1;
  const raw: string[][] = Array.from({ length: lanes }, () => []);
  for (const part of shape.parts) raw[part.lane]!.push(part.id);
  const kept = raw
    .map((column, index) => ({ column, index }))
    .filter((entry) => entry.column.length);
  const columns = order(
    kept.map((entry) => entry.column),
    shape.flows,
  );
  const col = new Map<string, number>();
  columns.forEach((column, index) => column.forEach((id) => col.set(id, index)));
  const across = shape.flows.filter((flow) => col.get(flow.from) !== col.get(flow.to));
  const within = shape.flows.filter((flow) => col.get(flow.from) === col.get(flow.to));
  const twins = new Set(
    shape.flows
      .filter((flow) =>
        shape.flows.some((other) => other.from === flow.to && other.to === flow.from),
      )
      .map((flow) => flow.index),
  );

  /** The air under a part: room for the name of a flow that runs down to the next. */
  const under = (upper: string, lower: string) => {
    const named = within.filter(
      (flow) =>
        flow.label &&
        ((flow.from === upper && flow.to === lower) || (flow.from === lower && flow.to === upper)),
    );
    return Math.max(ROW, ...named.map((flow) => plateHeight(flow.label) + PLATE.air * 2));
  };
  // Rows: stacked, then drawn level with the parts they talk to, never overlapping.
  const y = new Map<string, number>();
  for (const column of columns) {
    let next = band;
    column.forEach((id, row) => {
      y.set(id, next);
      next += H + (column[row + 1] ? under(id, column[row + 1]!) : 0);
    });
  }
  for (let pass = 0; pass < 4; pass += 1)
    for (const column of columns) {
      const want = column.map((id) => {
        const near = across.flatMap((flow) =>
          flow.from === id ? [y.get(flow.to)!] : flow.to === id ? [y.get(flow.from)!] : [],
        );
        return near.length ? near.reduce((a, b) => a + b, 0) / near.length : y.get(id)!;
      });
      let floor = band;
      column.forEach((id, row) => {
        // A part drifts from the one above it by at most a part's height: bends cost less
        // than a column of empty canvas.
        const ceiling = row ? floor + H : Infinity;
        const next = Math.max(floor, Math.min(ceiling, Math.round(want[row]!)));
        y.set(id, next);
        floor = next + H + (column[row + 1] ? under(id, column[row + 1]!) : 0);
      });
    }
  const top = Math.min(...[...y.values()]);
  for (const [id, value] of y) y.set(id, value - top + band);

  // Gaps: a track per flow that turns in it, and room for the widest name that lands after it.
  const gapFlows = columns.map(() => [] as Flow[]);
  const namedIn = columns.map(() => 0);
  const target = (flow: Flow) => {
    const a = col.get(flow.from)!;
    const b = col.get(flow.to)!;
    return a < b ? b - 1 : b;
  };
  for (const flow of across) {
    const a = col.get(flow.from)!;
    const b = col.get(flow.to)!;
    const last = target(flow);
    gapFlows[last]!.push(flow);
    if (Math.abs(a - b) > 1) gapFlows[a < b ? a : a - 1]!.push(flow);
    if (flow.label) namedIn[last] = Math.max(namedIn[last]!, plateWidth(flow.label));
  }
  // A flow out beside its own column names itself in the gap after it.
  for (const flow of within)
    if (flow.label) {
      const index = col.get(flow.from)!;
      namedIn[index] = Math.max(namedIn[index]!, plateWidth(flow.label) / 2);
    }
  const gaps = columns.map((_, index) =>
    index === columns.length - 1
      ? namedIn[index]
        ? AIR + namedIn[index]!
        : 0
      : AIR +
        Math.max(1, new Set(gapFlows[index]!.map((flow) => flow.index)).size) * TRACK +
        AIR +
        namedIn[index]! +
        AIR,
  );
  const left: number[] = [];
  let x = 0;
  columns.forEach((_, index) => {
    left.push(x);
    x += W + gaps[index]!;
  });
  const at: Record<string, CanvasPosition> = {};
  for (const [id, column] of col) at[id] = { x: left[column]!, y: y.get(id)! };

  /**
   * Flows between neighbouring tiers that leave one part (or reach one part)
   * through the same gap are one bus: a shared trunk, one exit (or entry), a
   * branch to each end. What remains runs on a track of its own.
   */
  const bundle = new Map<number, string>();
  const firstGap = (flow: Flow) => {
    const a = col.get(flow.from)!;
    return a < col.get(flow.to)! ? a : a - 1;
  };
  const candidates = across.filter((flow) => !twins.has(flow.index));
  const count = (key: (flow: Flow) => string) => {
    const sizes = new Map<string, number>();
    for (const flow of candidates) sizes.set(key(flow), (sizes.get(key(flow)) ?? 0) + 1);
    return sizes;
  };
  const outKey = (flow: Flow) => `out:${flow.from}:${firstGap(flow)}`;
  const inKey = (flow: Flow) => `in:${flow.to}:${target(flow)}`;
  const outs = count(outKey);
  const ins = count(inKey);
  for (const flow of candidates)
    if (outs.get(outKey(flow))! > 1) bundle.set(flow.index, outKey(flow));
    else if (ins.get(inKey(flow))! > 1) bundle.set(flow.index, inKey(flow));
  /** A bus shares its trunk only in the gap it gathers in: out of its source, or into its target. */
  const unit = (flow: Flow, gap: number) => {
    const group = bundle.get(flow.index);
    if (group?.startsWith("out:") && gap === firstGap(flow)) return group;
    if (group?.startsWith("in:") && gap === target(flow)) return group;
    return `f:${flow.index}`;
  };

  // A track per bus or lone flow in each gap it turns in, in order of where it lands.
  const tracks = new Map<string, number>();
  columns.forEach((_, index) => {
    const through = [...new Map(gapFlows[index]!.map((flow) => [flow.index, flow])).values()];
    const units = new Map<string, Flow[]>();
    for (const flow of through)
      units.set(unit(flow, index), [...(units.get(unit(flow, index)) ?? []), flow]);
    const mean = (flows: Flow[], end: "to" | "from") =>
      flows.reduce((sum, flow) => sum + at[flow[end]]!.y, 0) / flows.length;
    [...units.values()]
      .sort((a, b) => mean(a, "to") - mean(b, "to") || mean(a, "from") - mean(b, "from"))
      .forEach((flows, row) => {
        for (const flow of flows)
          tracks.set(`${index}:${flow.index}`, left[index]! + W + AIR + row * TRACK);
      });
  });
  /** Rows already taken by a flow passing over tiers, so no two run on one line. */
  const passes: { y: number; from: number; to: number }[] = [];
  /** A row across the columns between two, clear of their parts and of other passing flows. */
  const clearRow = (from: number, to: number, near: number) => {
    const low = Math.min(from, to);
    const high = Math.max(from, to);
    const blocked = (value: number) =>
      columns
        .slice(low + 1, high)
        .some((column) =>
          column.some((id) => value > at[id]!.y - 10 && value < at[id]!.y + H + 10),
        ) ||
      passes.some(
        (pass) => pass.from < high && low < pass.to && Math.abs(pass.y - value) < TRACK + 4,
      );
    const take = (value: number) => {
      passes.push({ y: value, from: low, to: high });
      return value;
    };
    if (!blocked(near)) return take(near);
    for (let step = 1; step < 80; step += 1)
      for (const value of [near + step * 8, near - step * 8])
        if (value >= 4 && !blocked(value)) return take(value);
    return take(near);
  };

  /**
   * Where each flow meets its part: flows sharing a side of a part spread over
   * its middle, ordered by where their other end stands, so no two share a run.
   */
  const ports = new Map<string, number>();
  const sides = new Map<string, Map<string, number[]>>();
  const portKey = (flow: Flow, end: "from" | "to") => {
    const group = bundle.get(flow.index);
    return group && group.startsWith(end === "from" ? "out:" : "in:") ? group : `f:${flow.index}`;
  };
  for (const flow of across) {
    if (twins.has(flow.index)) continue;
    const forward = col.get(flow.from)! < col.get(flow.to)!;
    const ends = [
      [`${flow.from}:${forward ? "right" : "left"}`, portKey(flow, "from"), at[flow.to]!.y],
      [`${flow.to}:${forward ? "left" : "right"}`, portKey(flow, "to"), at[flow.from]!.y],
    ] as const;
    for (const [side, key, other] of ends) {
      const entries = sides.get(side) ?? new Map<string, number[]>();
      entries.set(key, [...(entries.get(key) ?? []), other]);
      sides.set(side, entries);
    }
  }
  for (const [side, entries] of sides) {
    const list = [...entries].map(([key, others]) => ({
      key,
      other: others.reduce((a, b) => a + b, 0) / others.length,
    }));
    list.sort((a, b) => a.other - b.other || a.key.localeCompare(b.key));
    const step = Math.min(PORT, (H - 2 * PORT_EDGE) / Math.max(1, list.length - 1));
    list.forEach((entry, index) =>
      ports.set(`${side}:${entry.key}`, (index - (list.length - 1) / 2) * step),
    );
  }
  const port = (id: string, side: "left" | "right", flow: Flow) => {
    const forward = col.get(flow.from)! < col.get(flow.to)!;
    const end = id === flow.from && side === (forward ? "right" : "left") ? "from" : "to";
    return ports.get(`${id}:${side}:${portKey(flow, end)}`) ?? 0;
  };

  const primary = primaryFlows(shape);
  const flows: Record<number, DiagramFlow> = {};
  for (const flow of across) {
    const a = col.get(flow.from)!;
    const b = col.get(flow.to)!;
    const forward = a < b;
    const shiftY = twins.has(flow.index) ? (forward ? -TWIN : TWIN) : 0;
    const sy =
      at[flow.from]!.y + H / 2 + shiftY + port(flow.from, forward ? "right" : "left", flow);
    const ty = at[flow.to]!.y + H / 2 + shiftY + port(flow.to, forward ? "left" : "right", flow);
    const sx = forward ? at[flow.from]!.x + W : at[flow.from]!.x;
    const tx = forward ? at[flow.to]!.x : at[flow.to]!.x + W;
    const last = target(flow);
    const lastTrack = tracks.get(`${last}:${flow.index}`)!;
    let points: CanvasPosition[];
    if (Math.abs(a - b) === 1) {
      points = [
        { x: sx, y: sy },
        { x: lastTrack, y: sy },
        { x: lastTrack, y: ty },
        { x: tx, y: ty },
      ];
    } else {
      const firstGap = forward ? a : a - 1;
      const firstTrack = tracks.get(`${firstGap}:${flow.index}`)!;
      const pass = clearRow(a, b, sy);
      points = [
        { x: sx, y: sy },
        { x: firstTrack, y: sy },
        { x: firstTrack, y: pass },
        { x: lastTrack, y: pass },
        { x: lastTrack, y: ty },
        { x: tx, y: ty },
      ];
    }
    points = points.filter(
      (point, index) =>
        index === 0 || point.x !== points[index - 1]!.x || point.y !== points[index - 1]!.y,
    );
    const run = points.slice(-2);
    const plate = flow.label
      ? {
          x: (run[0]!.x + run[1]!.x) / 2 + (forward ? AIR / 2 : -AIR / 2),
          y:
            ty +
            (twins.has(flow.index) ? (forward ? -1 : 1) * (plateHeight(flow.label) / 2 + 1) : 0),
        }
      : undefined;
    flows[flow.index] = {
      from: flow.from,
      to: flow.to,
      points,
      ...(plate ? { plate } : {}),
      primary: primary.has(flow.index),
    };
  }
  // Inside a tier: straight down to the next part, or out beside the column and back.
  for (const flow of within) {
    const a = at[flow.from]!;
    const b = at[flow.to]!;
    const column = columns[col.get(flow.from)!]!;
    const between = column.some((id) => {
      const p = at[id]!;
      return p !== a && p !== b && p.y > Math.min(a.y, b.y) && p.y < Math.max(a.y, b.y);
    });
    const twin = twins.has(flow.index) ? (a.y < b.y ? -TWIN : TWIN) : 0;
    const points = !between
      ? a.y < b.y
        ? [
            { x: a.x + W / 2 + twin, y: a.y + H },
            { x: a.x + W / 2 + twin, y: b.y },
          ]
        : [
            { x: a.x + W / 2 + twin, y: a.y },
            { x: a.x + W / 2 + twin, y: b.y + H },
          ]
      : (() => {
          const out = a.x + W + AIR + (flow.label ? plateWidth(flow.label) / 2 : 0) - twin;
          return [
            { x: a.x + W, y: a.y + H / 2 },
            { x: out, y: a.y + H / 2 },
            { x: out, y: b.y + H / 2 },
            { x: b.x + W, y: b.y + H / 2 },
          ];
        })();
    const mid = midpoint(points);
    flows[flow.index] = {
      from: flow.from,
      to: flow.to,
      points,
      ...(flow.label
        ? {
            plate: {
              x: mid.x + (twin ? (plateWidth(flow.label) / 2 + TWIN) * Math.sign(twin) : 0),
              y: mid.y,
            },
          }
        : {}),
      primary: primary.has(flow.index),
    };
  }
  const width = Math.max(
    ...Object.values(at).map((p) => p.x + W),
    ...Object.values(flows).flatMap((flow) => flow.points.map((point) => point.x)),
    ...Object.values(flows).flatMap((flow) =>
      flow.plate
        ? [
            flow.plate.x +
              plateWidth(
                shape.flows.find((entry) => entry.from === flow.from && entry.to === flow.to)
                  ?.label ?? "",
              ) /
                2,
          ]
        : [],
    ),
  );
  const height = Math.max(...Object.values(at).map((p) => p.y + H)) + PLATE.air;
  const bounds = { x: 0, y: 0, width, height };
  return {
    at,
    width,
    height,
    bounds,
    layers: lanesOf(shape),
    bands: bandsOf(shape, at, bounds, 0),
    flows,
    settled: true,
  };
}
