import type { DiagramShape } from "./diagram";
import { DIAGRAM } from "./diagram-metrics";

/**
 * The rows a diagram reads in, top to bottom: tiers in their stated order,
 * and within a tier each part one row below the furthest part of its tier
 * that flows into it. A row holds at most `across` parts; a wider one wraps.
 */
export type DiagramRows = {
  rows: string[][];
  rowOf: Map<string, number>;
  /** Each tier's lane and its first and last row. */
  tiers: { lane: number; first: number; last: number }[];
  /** Rows that open a tier after the first. */
  tierStart: Set<number>;
  /** Whether a flow runs down the picture; one that does not is drawn back up. */
  forward: Map<number, boolean>;
};

export function diagramRows(shape: DiagramShape, across: number = DIAGRAM.across): DiagramRows {
  const order = new Map(shape.parts.map((part, index) => [part.id, index]));
  const lanes = [...new Set(shape.parts.map((part) => part.lane))].sort((a, b) => a - b);
  const laneOf = new Map(shape.parts.map((part) => [part.id, part.lane]));
  const rows: string[][] = [];
  const tiers: DiagramRows["tiers"] = [];
  const tierStart = new Set<number>();
  for (const lane of lanes) {
    const ids = shape.parts.filter((part) => part.lane === lane).map((part) => part.id);
    const inside = shape.flows.filter(
      (flow) => laneOf.get(flow.from) === lane && laneOf.get(flow.to) === lane,
    );
    const ranks = rank(ids, inside, order);
    const first = rows.length;
    const byRank: string[][] = [];
    for (const id of ids) (byRank[ranks.get(id)!] ??= []).push(id);
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
  const forward = new Map(
    shape.flows.map((flow) => [flow.index, rowOf.get(flow.from)! < rowOf.get(flow.to)!]),
  );
  return { rows, rowOf, tiers, tierStart, forward };
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
