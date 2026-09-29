import type { CanvasPosition } from "./canvas-model";
import type { DiagramShape } from "./diagram";
import type { DiagramRows } from "./diagram-rows";

/*
 * Routing happens in the picture's own frame: `x` runs across a row, `y`
 * along the way the picture reads (down, or right once turned). Everything
 * here is the same for both ways; only the parts' extents differ.
 */

/**
 * Flows drawn as one line that branches: a trunk, one run in the air above
 * each row it reaches, a branch down into each target. `down` gathers the
 * flows down the picture from its roots: one part's, or several parts' that
 * reach mostly the same parts (see `bundles`). `back` gathers the flows
 * drawn up into one part (its replies), rooted at that part. `pair` gathers
 * the flows between one part and those below it that flow back too, each
 * drawn once with a head at each end.
 */
export type Tree = {
  kind: "down" | "back" | "pair";
  /** The upper ends: where the lines start. */
  roots: string[];
  /** Each flow: its index, its lower end, and the root it comes down from. */
  leaves: { index: number; part: string; root: string }[];
};

export type Plan = {
  trees: Tree[];
  /** Flows into a part set beside their source: one short line across. */
  lateral: { index: number; from: string; to: string }[];
  /** The other way of a pair, by its partner's index. */
  twins: Map<number, number>;
};

type Flow = DiagramShape["flows"][number];

export function planOf(shape: DiagramShape, rows: DiagramRows): Plan {
  const row = (id: string) => rows.rowOf.get(id)!;
  const keys = new Set(shape.flows.map((flow) => `${flow.from}\n${flow.to}`));
  const twins = new Map<number, number>();
  const leads = new Map<string, number>();
  const trees: Tree[] = [];
  const singles = new Map<string, Tree>();
  const lateral: Plan["lateral"] = [];
  const down: Flow[] = [];
  const add = (key: string, kind: Tree["kind"], root: string, index: number, part: string) => {
    let tree = singles.get(key);
    if (!tree) {
      tree = { kind, roots: [root], leaves: [] };
      singles.set(key, tree);
      trees.push(tree);
    }
    tree.leaves.push({ index, part, root });
  };
  for (const flow of shape.flows) {
    const [from, to] = [row(flow.from), row(flow.to)];
    if (from === to) {
      lateral.push({ index: flow.index, from: flow.from, to: flow.to });
      continue;
    }
    const partner = leads.get(`${flow.to}\n${flow.from}`);
    if (partner !== undefined) {
      twins.set(flow.index, partner);
      continue;
    }
    if (keys.has(`${flow.to}\n${flow.from}`)) {
      leads.set(`${flow.from}\n${flow.to}`, flow.index);
      const [upper, lower] = from < to ? [flow.from, flow.to] : [flow.to, flow.from];
      add(`pair|${upper}`, "pair", upper, flow.index, lower);
    } else if (from < to) down.push(flow);
    else add(`back|${flow.to}`, "back", flow.to, flow.index, flow.from);
  }
  const bundled = bundles(down, row);
  for (const bundle of bundled) trees.push(bundle);
  const taken = new Set(bundled.flatMap((tree) => tree.leaves.map((leaf) => leaf.index)));
  for (const flow of down)
    if (!taken.has(flow.index)) add(`down|${flow.from}`, "down", flow.from, flow.index, flow.to);
  return { trees, lateral, twins };
}

/**
 * Parts whose lines down reach mostly the same parts, joined into one tree:
 * their trunks become one where the lower of them stands, and it branches to
 * every part either reaches. A part keeps one trunk; two join when at least
 * two parts below the join are reached by both and those are at least half
 * of what either reaches there. Read at rest the joined line may lead from
 * one to a part only the other reaches; its hover and its popover say which.
 */
function bundles(down: readonly Flow[], row: (id: string) => number): Tree[] {
  const order: string[] = [];
  const targets = new Map<string, Set<string>>();
  for (const flow of down) {
    if (!targets.has(flow.from)) order.push(flow.from);
    targets.set(flow.from, (targets.get(flow.from) ?? new Set()).add(flow.to));
  }
  const groups = order.map((id) => [id]);
  const alike = (a: string[], b: string[]) => {
    const join = Math.max(...[...a, ...b].map(row));
    const below = (group: string[]) =>
      new Set(group.flatMap((id) => [...targets.get(id)!]).filter((to) => row(to) > join));
    const [x, y] = [below(a), below(b)];
    const both = [...x].filter((to) => y.has(to)).length;
    return both >= 2 && both / new Set([...x, ...y]).size >= 0.5;
  };
  for (let merged = true; merged;) {
    merged = false;
    for (let i = 0; i < groups.length && !merged; i += 1)
      for (let j = i + 1; j < groups.length && !merged; j += 1)
        if (alike(groups[i]!, groups[j]!)) {
          groups[i] = [...groups[i]!, ...groups[j]!];
          groups.splice(j, 1);
          merged = true;
        }
  }
  return groups
    .filter((group) => group.length > 1)
    .map((group) => ({
      kind: "down" as const,
      roots: [...group].sort((a, b) => row(a) - row(b) || order.indexOf(a) - order.indexOf(b)),
      leaves: down
        .filter((flow) => group.includes(flow.from))
        .map((flow) => ({ index: flow.index, part: flow.to, root: flow.from })),
    }));
}

/** A tree's first row: where its highest root stands. */
export const topOf = (tree: Tree, rows: DiagramRows) =>
  Math.min(...tree.roots.map((id) => rows.rowOf.get(id)!));

/** The rows a tree runs through between its first row and its deepest leaf: where its trunk passes. */
export function passes(tree: Tree, rows: DiagramRows): number[] {
  const top = topOf(tree, rows);
  const deepest = Math.max(...tree.leaves.map((leaf) => rows.rowOf.get(leaf.part)!));
  return Array.from({ length: Math.max(0, deepest - top - 1) }, (_, at) => top + 1 + at);
}

/**
 * A horizontal run in the air between two rows: lines come down into it at
 * `stems`, leave it downward at `drops`, and it spans `lo`–`hi` at `level`.
 */
type Bus = { stems: number[]; drops: number[]; lo: number; hi: number; level: number };

const MARGIN = 10;
const STRAIGHT = 0.5;
const same = (a: number, b: number) => Math.abs(a - b) < STRAIGHT;
const inside = (bus: Bus, u: number) => u > bus.lo + STRAIGHT && u < bus.hi - STRAIGHT;
const flat = (bus: Bus) => bus.hi - bus.lo < STRAIGHT;
const meet = (a: Bus, b: Bus) => a.lo < b.hi + MARGIN && b.lo < a.hi + MARGIN;
const bus = (stems: number[], drops: number[]): Bus => ({
  stems,
  drops,
  lo: Math.min(...stems, ...drops),
  hi: Math.max(...stems, ...drops),
  level: 0,
});

/**
 * What it costs to set `a` above `b` in one air: a branch of `a` crossing
 * `b`'s run, a stem of `b` crossing `a`'s, and (never) a branch of `a`
 * running on a stem of `b`. Branches into one port join; they cost nothing.
 */
function cost(a: Bus, b: Bus): number {
  let sum = 0;
  for (const drop of a.drops) {
    if (b.stems.some((stem) => same(stem, drop))) sum += 100;
    else if (inside(b, drop) && !b.drops.some((other) => same(other, drop))) sum += 1;
  }
  for (const stem of b.stems) if (inside(a, stem)) sum += 1;
  return sum;
}

function* orders(n: number): Generator<number[]> {
  const order = Array.from({ length: n }, (_, i) => i);
  const permute = function* (k: number): Generator<number[]> {
    if (k === n) {
      yield order;
      return;
    }
    for (let i = k; i < n; i += 1) {
      [order[k], order[i]] = [order[i]!, order[k]!];
      yield* permute(k + 1);
      [order[k], order[i]] = [order[i]!, order[k]!];
    }
  };
  yield* permute(0);
}

/**
 * The levels a set of runs take, from `from`: an order that crosses least
 * (every order when there are few, the stems' order otherwise), then each
 * run on the first level clear of those before it that it would meet.
 * Returns the number of levels used.
 */
function stack(buses: readonly Bus[], from: number): number {
  const wide = buses.filter((entry) => !flat(entry));
  if (!wide.length) return 0;
  const levelsOf = (order: readonly number[]) => {
    const levels: number[] = [];
    order.forEach((index, at) => {
      let level = 0;
      for (const earlier of order.slice(0, at))
        if (meet(wide[index]!, wide[earlier]!)) level = Math.max(level, levels[earlier]! + 1);
      levels[index] = level;
    });
    return levels;
  };
  const score = (order: readonly number[]) => {
    let sum = 0;
    for (let i = 0; i < order.length; i += 1)
      for (let j = i + 1; j < order.length; j += 1) {
        const [a, b] = [wide[order[i]!]!, wide[order[j]!]!];
        if (meet(a, b)) sum += cost(a, b);
      }
    return sum;
  };
  let best: { order: number[]; score: number; depth: number } | null = null;
  const consider = (order: readonly number[]) => {
    const value = score(order);
    const depth = Math.max(...levelsOf(order));
    if (!best || value < best.score || (value === best.score && depth < best.depth))
      best = { order: [...order], score: value, depth };
  };
  if (wide.length <= 6) for (const order of orders(wide.length)) consider(order);
  else
    consider(
      wide
        .map((entry, index) => ({ at: Math.min(...entry.stems), index }))
        .sort((a, b) => a.at - b.at)
        .map((entry) => entry.index),
    );
  const levels = levelsOf(best!.order);
  wide.forEach((entry, index) => (entry.level = from + levels[index]!));
  return Math.max(...levels) + 1;
}

/**
 * One tree's step across the air after a row: its lines come down at
 * `stems` (its trunk, and the ports of its roots in that row) and leave for
 * each of `drops`, a part's port or its trunk going on through the next row.
 * Drops are keyed by where they land: two trees into one port share its key.
 */
export type Step = { tree: number; stems: number[]; drops: { key: string; at: number }[] };

/**
 * The runs across one air. Each tree's branches leave its stems on its own
 * run. Where two or more trees reach the same two or more ports, their stems
 * join one run that drops into each: one line where there would be a
 * bundle, true as long as it is read down (a stem met on the way goes back
 * up). Own runs stand above the joined ones, so nothing that is one tree's
 * alone hangs from a shared run. Returns the levels used and, by
 * `tree|drop key`, the level each line crosses at (`null`: straight down).
 */
export function routeAir(steps: readonly Step[]): {
  levels: number;
  level: Map<string, number | null>;
} {
  const reach = new Map<string, Set<number>>();
  const where = new Map<string, number>();
  for (const step of steps)
    for (const drop of step.drops)
      if (drop.key.startsWith("p")) {
        reach.set(drop.key, (reach.get(drop.key) ?? new Set()).add(step.tree));
        where.set(drop.key, drop.at);
      }
  // Ports reached by the same trees, largest groups first; a tree joins once.
  const grouped = new Map<string, { trees: number[]; keys: string[] }>();
  for (const [key, trees] of reach) {
    if (trees.size < 2) continue;
    const list = [...trees].sort((a, b) => a - b);
    const id = list.join(",");
    const group = grouped.get(id) ?? { trees: list, keys: [] };
    group.keys.push(key);
    grouped.set(id, group);
  }
  const used = new Set<number>();
  const merges = [...grouped.values()]
    .filter((group) => group.keys.length >= 2)
    .sort((a, b) => b.trees.length * b.keys.length - a.trees.length * a.keys.length)
    .filter((group) => {
      if (group.trees.some((tree) => used.has(tree))) return false;
      for (const tree of group.trees) used.add(tree);
      return true;
    });
  const merged = new Set(
    merges.flatMap((group) =>
      group.trees.flatMap((tree) => group.keys.map((key) => `${tree}|${key}`)),
    ),
  );
  const stemsOf = new Map(steps.map((step) => [step.tree, step.stems]));
  const own = steps.flatMap((step) => {
    const mine = step.drops.filter((drop) => !merged.has(`${step.tree}|${drop.key}`));
    return mine.length
      ? [
          {
            step,
            run: bus(
              step.stems,
              mine.map((drop) => drop.at),
            ),
            keys: mine,
          },
        ]
      : [];
  });
  const joins = merges.map((group) => ({
    group,
    run: bus(
      group.trees.flatMap((tree) => stemsOf.get(tree)!),
      group.keys.map((key) => where.get(key)!),
    ),
  }));
  const ownLevels = stack(
    own.map((entry) => entry.run),
    0,
  );
  const joinLevels = stack(
    joins.map((entry) => entry.run),
    ownLevels,
  );
  const level = new Map<string, number | null>();
  for (const { step, run, keys } of own)
    for (const drop of keys) level.set(`${step.tree}|${drop.key}`, flat(run) ? null : run.level);
  for (const { group, run } of joins)
    for (const tree of group.trees)
      for (const key of group.keys) level.set(`${tree}|${key}`, run.level);
  return { levels: ownLevels + joinLevels, level };
}

/** A line with its junctions kept: a point wherever another line joins or leaves it. */
export function joined(lines: readonly CanvasPosition[][]): CanvasPosition[][] {
  return lines.map((line, own) => {
    const out: CanvasPosition[] = [line[0]!];
    for (let i = 1; i < line.length; i += 1) {
      const [a, b] = [line[i - 1]!, line[i]!];
      const along = same(a.x, b.x) ? "y" : "x";
      const lo = Math.min(a[along], b[along]);
      const hi = Math.max(a[along], b[along]);
      const across = along === "y" ? "x" : "y";
      const cuts = lines
        .flatMap((other, index) => (index === own ? [] : other))
        .filter(
          (p) => same(p[across], a[across]) && p[along] > lo + STRAIGHT && p[along] < hi - STRAIGHT,
        )
        .map((p) => p[along]);
      const sorted = [...new Set(cuts.map((value) => Math.round(value * 10) / 10))].sort((x, y) =>
        a[along] < b[along] ? x - y : y - x,
      );
      for (const value of sorted)
        out.push(along === "y" ? { x: a.x, y: value } : { x: value, y: a.y });
      out.push(b);
    }
    return out;
  });
}
