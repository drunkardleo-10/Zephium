import type { CanvasPosition } from "./canvas-model";
import { clean, type DiagramShape } from "./diagram";
import { DIAGRAM } from "./diagram-metrics";
import type { DiagramRows } from "./diagram-rows";
import type { Route } from "./diagram-worker";

/** Strands in a channel stand this far apart; the first this far from the parts. */
const LANE = 20;
const OFF = 22;
/** Where a strand meets a part's top or bottom: this far in from its corner. */
const INSET = 20;
/** A strand's crossing run keeps this far from any other run along it. */
const CLEAR = 12;

type Side = "left" | "right";
type Run = { y: number; from: number; to: number };

/**
 * Flows past the next row, drawn outside the parts. Each part's flows that
 * way leave it as one strand, down (or up) a channel of their own beside the
 * picture, left or right, whichever side is nearer; they turn in to their
 * targets in the air above (or below) the target's row, or straight into its
 * side when nothing stands between. Strands into one target on one side meet
 * before it and arrive as one. Every crossing run keeps clear of the others.
 * Returns where the parts stand once the channels have room, the strands by
 * the route they draw, and how far everything moved right.
 */
export function channels(
  shape: DiagramShape,
  rows: DiagramRows,
  placed: Record<string, CanvasPosition>,
  routes: readonly Route[],
  lines: readonly (readonly CanvasPosition[])[],
  gutter: number,
): { shift: number; at: Record<string, CanvasPosition>; lines: Map<number, CanvasPosition[]> } {
  const { width: W, height: H } = DIAGRAM.node;
  const out = new Map<number, CanvasPosition[]>();
  if (!routes.length) return { shift: 0, at: placed, lines: out };
  const at = placed;
  const top = rows.rows.map((row) => at[row[0]!]!.y);
  const row = (id: string) => rows.rowOf.get(id)!;
  const xs = [
    ...Object.values(at).flatMap((point) => [point.x, point.x + W]),
    ...lines.flatMap((line) => line.map((point) => point.x)),
  ];
  const min = Math.min(...xs);
  const max = Math.max(...xs);
  const middle = (min + max) / 2;

  // Runs across the picture already drawn, so a new one keeps clear of them.
  const runs: Run[] = [];
  for (const line of lines)
    line.slice(1).forEach((point, index) => {
      const before = line[index]!;
      if (Math.abs(point.y - before.y) < 0.5)
        runs.push({
          y: point.y,
          from: Math.min(point.x, before.x),
          to: Math.max(point.x, before.x),
        });
    });
  /** A clear height for a run across `from`–`to` in the air between two rows. */
  function track(above: number, below: number, from: number, to: number): number {
    const lo = above + H + 8;
    const hi = below - 8;
    const mid = (lo + hi) / 2;
    const free = (y: number) =>
      runs.every(
        // Two runs on one height read as one line even with a gap between: keep them apart.
        (run) => run.to < from - 32 || run.from > to + 32 || Math.abs(run.y - y) >= CLEAR,
      );
    let y = mid;
    for (let step = 0; step < 40; step += 1) {
      const candidate = mid + (step % 2 ? 1 : -1) * Math.ceil(step / 2) * 4;
      if (candidate >= lo && candidate <= hi && free(candidate)) {
        y = candidate;
        break;
      }
    }
    runs.push({ y, from, to });
    return y;
  }
  /** Nothing in the part's row stands between it and that side's channel. */
  const open = (id: string, side: Side) =>
    rows.rows[row(id)]!.every(
      (other) =>
        other === id || (side === "left" ? at[other]!.x > at[id]!.x : at[other]!.x < at[id]!.x),
    );

  type Leg = { route: Route; to: string };
  type Strand = { from: string; down: boolean; legs: Leg[]; side: Side; x: number };
  const strands = new Map<string, Strand>();
  for (const route of routes) {
    const flow = shape.flows.find((entry) => entry.index === route.lead)!;
    const down = row(flow.to) > row(flow.from);
    const key = `${flow.from}|${down}`;
    const strand = strands.get(key) ?? { from: flow.from, down, legs: [], side: "left", x: 0 };
    strand.legs.push({ route, to: flow.to });
    strands.set(key, strand);
  }
  const centre = (id: string) => at[id]!.x + W / 2;
  for (const strand of strands.values()) {
    const targets = strand.legs.reduce((sum, leg) => sum + centre(leg.to), 0) / strand.legs.length;
    strand.side = (centre(strand.from) + targets) / 2 <= middle ? "left" : "right";
  }
  // Strands bound for one part alone, on one side, share a lane: they join as they reach it
  // and arrive as one. The shorter a lane's reach, the nearer the parts it runs.
  const lanes = new Map<string, Strand[]>();
  for (const strand of strands.values()) {
    // A pair runs both ways; it keeps a lane of its own rather than join one-way lines.
    const only =
      strand.legs.every((leg) => leg.to === strand.legs[0]!.to) &&
      strand.legs.every((leg) => leg.route.partner === undefined);
    const key = only
      ? `to|${strand.legs[0]!.to}|${strand.side}|${strand.down}`
      : `from|${strand.from}|${strand.down}`;
    lanes.set(key, [...(lanes.get(key) ?? []), strand]);
  }
  const reach = (group: Strand[]) =>
    Math.max(
      ...group.flatMap((strand) =>
        strand.legs.map((leg) => Math.abs(row(leg.to) - row(strand.from))),
      ),
    );
  for (const side of ["left", "right"] as const)
    [...lanes.values()]
      .filter((group) => group[0]!.side === side)
      .sort((a, b) => reach(a) - reach(b))
      .forEach((group, lane) => {
        for (const strand of group)
          strand.x = side === "left" ? min - OFF - lane * LANE : max + OFF + lane * LANE;
      });

  const arrivals = new Map<string, number>();
  for (const strand of strands.values()) {
    const s = at[strand.from]!;
    const r = row(strand.from);
    const left = strand.side === "left";
    let leave: CanvasPosition[];
    if (open(strand.from, strand.side)) {
      const y = s.y + H * (strand.down ? 0.64 : 0.36);
      leave = [{ x: left ? s.x : s.x + W, y }];
    } else {
      const x = left ? s.x + INSET : s.x + W - INSET;
      const lo = Math.min(x, strand.x);
      const hi = Math.max(x, strand.x);
      const y = strand.down
        ? track(top[r]!, top[r + 1]!, lo, hi)
        : track(top[r - 1]!, top[r]!, lo, hi);
      leave = [
        { x, y: strand.down ? s.y + H : s.y },
        { x, y },
      ];
    }
    const turn = leave.at(-1)!;
    for (const leg of strand.legs) {
      const t = at[leg.to]!;
      const rt = row(leg.to);
      const key = `${leg.to}|${strand.side}|${strand.down}`;
      let arrive: CanvasPosition[];
      if (open(leg.to, strand.side)) {
        const y = t.y + H * (strand.down ? 0.36 : 0.64);
        arrive = [
          { x: strand.x, y },
          { x: left ? t.x : t.x + W, y },
        ];
      } else {
        const x = left ? t.x + INSET : t.x + W - INSET;
        let y = arrivals.get(key);
        if (y === undefined) {
          const lo = Math.min(x, strand.x) - (left ? LANE * 4 : 0);
          const hi = Math.max(x, strand.x) + (left ? 0 : LANE * 4);
          y = strand.down
            ? track(top[rt - 1]!, top[rt]!, lo, hi)
            : track(top[rt]!, top[rt + 1]!, lo, hi);
          arrivals.set(key, y);
        }
        arrive = [
          { x: strand.x, y },
          { x, y },
          { x, y: strand.down ? t.y : t.y + H },
        ];
      }
      const line = clean([...leave, { x: strand.x, y: turn.y }, ...arrive]);
      // Handed back as the engine draws: from the upper part down.
      out.set(leg.route.lead, strand.down ? line : line.reverse());
    }
  }
  // Room for the left channels beside the tiers' names.
  const leftmost = Math.min(min, ...[...strands.values()].map((strand) => strand.x));
  const shift = Math.max(0, (gutter ? gutter + 8 : 0) - leftmost);
  const moved = (point: CanvasPosition) => ({ x: point.x + shift, y: point.y });
  return {
    shift,
    at: Object.fromEntries(Object.entries(at).map(([id, point]) => [id, moved(point)])),
    lines: new Map([...out].map(([lead, line]) => [lead, line.map(moved)])),
  };
}
