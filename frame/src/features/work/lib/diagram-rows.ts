import type { DiagramShape } from "./diagram";
import { DIAGRAM } from "./diagram-metrics";

/**
 * The rows a diagram reads in, along the way it reads: tiers in their stated
 * order, and within a tier each part one row past the furthest part of its
 * tier that flows into it. A row holds at most `across` parts; a wider one
 * wraps. A leaf that would take the tier's last row alone, fed by one part
 * that also flows elsewhere, stands beside that part instead (`beside`),
 * unless a line has to pass that part's row.
 */
export type DiagramRows = {
  rows: string[][];
  /** Each part's row, a part set beside another included. */
  rowOf: Map<string, number>;
  /** Parts set beside the one part that flows into them: before it and after it in its row. */
  beside: Map<string, { before?: string; after?: string }>;
  /** Each tier's lane and its first and last row. */
  tiers: { lane: number; first: number; last: number }[];
  /** Rows that open a tier after the first. */
  tierStart: Set<number>;
  /** Whether a flow runs down the picture; one that does not is drawn back up. */
  forward: Map<number, boolean>;
};

export function diagramRows(
  shape: DiagramShape,
  across: number = DIAGRAM.across,
  apart: ReadonlySet<number> = new Set(),
): DiagramRows {
  const order = new Map(shape.parts.map((part, index) => [part.id, index]));
  const lanes = [...new Set(shape.parts.map((part) => part.lane))].sort((a, b) => a - b);
  const laneOf = new Map(shape.parts.map((part) => [part.id, part.lane]));
  const rows: string[][] = [];
  const tiers: DiagramRows["tiers"] = [];
  const tierStart = new Set<number>();
  const beside = new Map<string, { before?: string; after?: string }>();
  const into = new Map<string, string[]>();
  const out = new Map<string, number>();
  for (const flow of shape.flows) {
    into.set(flow.to, [...(into.get(flow.to) ?? []), flow.from]);
    out.set(flow.from, (out.get(flow.from) ?? 0) + 1);
  }
  for (const lane of lanes) {
    const ids = shape.parts.filter((part) => part.lane === lane).map((part) => part.id);
    const inside = shape.flows.filter(
      (flow) => laneOf.get(flow.from) === lane && laneOf.get(flow.to) === lane,
    );
    const ranks = rank(ids, inside, order);
    const first = rows.length;
    const byRank: string[][] = [];
    for (const id of ids) (byRank[ranks.get(id)!] ??= []).push(id);
    const set = apart.has(lane) ? null : besides(byRank, into, out, across);
    if (set) {
      byRank.pop();
      for (const [parent, sides] of set) beside.set(parent, sides);
    }
    for (const group of byRank.filter(Boolean)) {
      const count = Math.ceil(group.length / across);
      const size = Math.ceil(group.length / count);
      for (let at = 0; at < group.length; at += size) rows.push(group.slice(at, at + size));
    }
    if (first && rows.length > first) tierStart.add(first);
    if (rows.length > first) tiers.push({ lane, first, last: rows.length - 1 });
  }
  const rowOf = new Map<string, number>();
  rows.forEach((row, index) => row.forEach((id) => rowOf.set(id, index)));
  for (const [parent, sides] of beside)
    for (const id of [sides.before, sides.after]) if (id) rowOf.set(id, rowOf.get(parent)!);
  const lateral = (from: string, to: string) =>
    beside.get(from)?.before === to || beside.get(from)?.after === to;
  const forward = new Map(
    shape.flows.map((flow) => [
      flow.index,
      rowOf.get(flow.from)! < rowOf.get(flow.to)! || lateral(flow.from, flow.to),
    ]),
  );
  // A part with a leaf beside it leaves its row no gap: where a line has to pass that row,
  // the leaf keeps a row of its own instead, so the line passes between parts.
  const crossed = new Set(
    [...beside.keys()]
      .filter((parent) =>
        shape.flows.some((flow) => {
          const [from, to] = [rowOf.get(flow.from)!, rowOf.get(flow.to)!];
          const at = rowOf.get(parent)!;
          return Math.min(from, to) < at && Math.max(from, to) > at;
        }),
      )
      .map((parent) => laneOf.get(parent)!),
  );
  if ([...crossed].some((lane) => !apart.has(lane)))
    return diagramRows(shape, across, new Set([...apart, ...crossed]));
  return { rows, rowOf, beside, tiers, tierStart, forward };
}

/**
 * The tier's last row set beside the parts that feed it, when every part in
 * it is a leaf fed by one part alone, from the row before, that also flows
 * elsewhere (a pipeline's end stays on the line), at most one each side of a
 * part, and the row before keeps within `across`.
 */
function besides(
  byRank: readonly (string[] | undefined)[],
  into: ReadonlyMap<string, string[]>,
  out: ReadonlyMap<string, number>,
  across: number,
): Map<string, { before?: string; after?: string }> | null {
  const last = byRank.at(-1);
  const before = byRank.at(-2);
  if (byRank.length < 2 || !last?.length || !before?.length) return null;
  if (before.length + last.length > across) return null;
  const set = new Map<string, { before?: string; after?: string }>();
  for (const id of last) {
    const from = into.get(id) ?? [];
    const parent = from[0];
    if (from.length !== 1 || out.get(id) || !parent || !before.includes(parent)) return null;
    const sides = set.get(parent) ?? {};
    if (!sides.after) sides.after = id;
    else if (!sides.before) sides.before = id;
    else return null;
    set.set(parent, sides);
  }
  for (const [parent, sides] of set)
    if ((out.get(parent) ?? 0) <= Number(!!sides.before) + Number(!!sides.after)) return null;
  return set;
}

/**
 * Each part's rank in its tier: the longest path to it from the tier's
 * sources. A cycle is cut where it closes, walking in the parts' own order.
 */
function rank(
  ids: readonly string[],
  flows: DiagramShape["flows"],
  order: ReadonlyMap<string, number>,
): Map<string, number> {
  const outgoing = new Map(ids.map((id) => [id, [] as string[]]));
  const incoming = new Map(ids.map((id) => [id, 0]));
  for (const flow of flows) {
    outgoing.get(flow.from)!.push(flow.to);
    incoming.set(flow.to, incoming.get(flow.to)! + 1);
  }
  const forward = new Map(ids.map((id) => [id, [] as string[]]));
  const seen = new Map<string, "open" | "done">();
  const visit = (id: string) => {
    seen.set(id, "open");
    for (const next of outgoing.get(id)!) {
      if (seen.get(next) === "open") continue;
      forward.get(id)!.push(next);
      if (!seen.has(next)) visit(next);
    }
    seen.set(id, "done");
  };
  const byOrder = [...ids].sort((a, b) => order.get(a)! - order.get(b)!);
  for (const id of [...byOrder.filter((id) => !incoming.get(id)), ...byOrder])
    if (!seen.has(id)) visit(id);
  const ranks = new Map(ids.map((id) => [id, 0]));
  const waiting = new Map(ids.map((id) => [id, 0]));
  for (const targets of forward.values())
    for (const target of targets) waiting.set(target, waiting.get(target)! + 1);
  const ready = byOrder.filter((id) => !waiting.get(id));
  while (ready.length) {
    const id = ready.shift()!;
    for (const next of forward.get(id)!) {
      ranks.set(next, Math.max(ranks.get(next)!, ranks.get(id)! + 1));
      waiting.set(next, waiting.get(next)! - 1);
      if (!waiting.get(next)) ready.push(next);
    }
  }
  return ranks;
}
