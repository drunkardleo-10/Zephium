import { describe, expect, test } from "vitest";
import { boardLayout, BOARD, type LayoutBlock } from "../lib/board/layout";
import { boardOf } from "../lib/board/adapter";
import { runTrail } from "../lib/board/trail";
import { DIAGRAM, diagramLayout } from "../lib/diagram";
import { environmentStages } from "../lib/project-environment-board";
import { siteKey } from "../lib/run/site";
import { viewPlacements } from "../lib/project-environment";
import type { BlockKind, Emphasis } from "../lib/board/types";
import {
  dinnerScene,
  jobsScene,
  runningScene,
  rustScene,
  saasScene,
  tripScene,
} from "./board-fixtures";

/** A small deterministic generator: the same seed, the same boards. */
function random(seed: number) {
  let state = seed;
  return () => {
    state = (state * 1103515245 + 12345) % 2147483648;
    return state / 2147483648;
  };
}
const KINDS: BlockKind[] = [
  "prose",
  "table",
  "chart",
  "gallery",
  "diagram",
  "checklist",
  "code",
  "entity",
  "document",
];
const EMPHASIS: Emphasis[] = ["hero", "primary", "primary", "supporting"];

describe("board layout", () => {
  test("no two blocks ever overlap, and every block stands inside a board 720–1280 wide", () => {
    for (let seed = 1; seed <= 400; seed += 1) {
      const next = random(seed);
      const count = 1 + Math.floor(next() * 11);
      const blocks: LayoutBlock[] = Array.from({ length: count }, (_, index) => {
        const min = 160 + Math.floor(next() * 900);
        const ideal = min + Math.floor(next() * 500);
        const max = ideal + Math.floor(next() * 800);
        const height = 60 + Math.floor(next() * 700);
        const group = next() < 0.25 ? `g${Math.floor(next() * 3)}` : undefined;
        return {
          id: `b${index}`,
          kind: KINDS[Math.floor(next() * KINDS.length)]!,
          emphasis: EMPHASIS[Math.floor(next() * EMPHASIS.length)]!,
          ...(group ? { group } : {}),
          width: { min, ideal, max },
          height: (width: number) => height + Math.round(10_000 / width),
        };
      });
      const open = next() < 0.3 ? blocks[Math.floor(next() * count)]!.id : undefined;
      const layout = boardLayout(blocks, open ? { open } : {});
      expect(layout.width).toBeGreaterThanOrEqual(BOARD.min);
      const wide = blocks.some((block) => block.emphasis === "hero" && block.kind === "diagram");
      expect(layout.width).toBeLessThanOrEqual(wide ? BOARD.wide : BOARD.max);
      const rects = Object.values(layout.at);
      expect(rects).toHaveLength(count);
      for (const rect of rects) {
        expect(rect.x).toBeGreaterThanOrEqual(0);
        expect(rect.x + rect.width).toBeLessThanOrEqual(layout.width);
        expect(rect.width).toBeGreaterThan(0);
      }
      for (const [index, a] of rects.entries())
        for (const b of rects.slice(index + 1))
          expect(
            a.x + a.width <= b.x ||
              b.x + b.width <= a.x ||
              a.y + a.height <= b.y ||
              b.y + b.height <= a.y,
          ).toBe(true);
    }
  });

  test("the hero leads at the board's width, a group stands together, supporting blocks trail", () => {
    const block = (id: string, emphasis: Emphasis, width: number, group?: string): LayoutBlock => ({
      id,
      kind: "table",
      emphasis,
      ...(group ? { group } : {}),
      width: { min: width - 100, ideal: width, max: width + 100 },
      height: () => 200,
    });
    const layout = boardLayout([
      block("aside", "supporting", 380),
      block("chart", "primary", 480, "pair"),
      block("diagram", "hero", 900),
      block("table", "primary", 520, "pair"),
    ]);
    expect(layout.at.diagram).toMatchObject({ x: 0, y: 0, width: layout.width });
    expect(layout.at.chart!.y).toBe(layout.at.table!.y);
    expect(layout.at.aside!.y).toBeGreaterThan(layout.at.table!.y);
    // A row shares its height, so the surfaces end together.
    expect(layout.at.chart!.height).toBe(layout.at.table!.height);
  });

  test("a block opened in place takes the board's width; one the person moved takes no room", () => {
    const block = (id: string): LayoutBlock => ({
      id,
      kind: "table",
      emphasis: "primary",
      width: { min: 360, ideal: 480, max: 560 },
      height: () => 200,
    });
    const blocks = [block("a"), block("b"), block("c")];
    const open = boardLayout(blocks, { open: "b" });
    expect(open.at.b).toMatchObject({ x: 0, width: open.width });
    expect(open.at.c!.y).toBeGreaterThan(open.at.b!.y);
    const pinned = boardLayout(blocks, { pinned: new Set(["a"]) });
    expect(pinned.at.a).toBeUndefined();
    expect(pinned.at.b).toMatchObject({ x: 0, y: 0 });
  });
});

const board = (scene: ReturnType<typeof saasScene>) => {
  const projection = [...scene.objectives.values()][0]!;
  return boardOf({
    id: "board",
    executions: projection.executions,
    elements: scene.snapshot.elements,
    pictures: scene.pictures,
  });
};

describe("the adapter", () => {
  test("an architecture: the answer's first sentence is the lead, the diagram leads, the cost chart holds its table", () => {
    const made = board(saasScene());
    expect(made.title).toBe("How to build a B2B SaaS that scales");
    expect(made.lead).toMatch(/^Start as a modular monolith .* file processing\.$/u);
    const kinds = made.blocks.map((block) => [block.kind, block.emphasis, block.title ?? ""]);
    expect(kinds).toContainEqual(["diagram", "hero", "Reference architecture"]);
    expect(kinds).toContainEqual(["prose", "primary", "How to build a B2B SaaS that scales"]);
    expect(kinds).toContainEqual(["checklist", "supporting", "Launch checklist"]);
    // The cost table and the cost chart carry the same rows: one block, the chart, its table as its values.
    expect(made.blocks.filter((block) => block.title === "Monthly cost by stage")).toHaveLength(1);
    const chart = made.blocks.find((block) => block.kind === "chart")!;
    if (chart.kind !== "chart") throw new Error("chart");
    expect(chart.values?.rows).toHaveLength(4);
    expect(chart.headline?.value).toBe("$50–$33,000");
    const stack = made.blocks.find((block) => block.title === "Recommended stack");
    expect(stack?.kind === "table" && stack.columns.map((column) => column.type)).toEqual([
      "text",
      "text",
      "long",
    ]);
    // No block is a findings list, whatever the run published.
    expect(made.blocks.every((block) => (block.kind as string) !== "findings")).toBe(true);
  });

  test("a trip: stays with pictures are the hero gallery and take their comparison; facts fold onto what they name", () => {
    const made = board(tripScene());
    const stays = made.blocks.find((block) => block.kind === "gallery" && block.facet === "stay");
    if (stays?.kind !== "gallery") throw new Error("stays");
    expect(stays.emphasis).toBe("hero");
    expect(stays.title).toBe("Stays near the YC office");
    expect(stays.compare?.subjects).toHaveLength(3);
    expect(stays.entities.every((entity) => !!entity.image)).toBe(true);
    expect(stays.entities[0]).toMatchObject({
      price: "$3,450.00",
      name: "Sunny flat near Caltrain",
    });
    expect(stays.entities[0]!.facts).toContainEqual(
      expect.objectContaining({ label: "", value: "Monthly stays get a 35% discount" }),
    );
    const flights = made.blocks.find(
      (block) => block.kind === "gallery" && block.facet === "flight",
    );
    expect(flights?.emphasis).toBe("primary");
    expect(made.blocks.filter((block) => block.kind === "comparison")).toEqual([]);
    // A fact no entity names closes the paragraph it speaks to.
    const prose = made.blocks.find((block) => block.kind === "prose");
    if (prose?.kind !== "prose") throw new Error("prose");
    const cited = Object.values(prose.cites).flat();
    expect(cited.map((key) => made.sources[key]?.origin)).toContain("esta.cbp.dhs.gov");
  });

  test("jobs are a gallery of roles; a rust explanation keeps its code; dinner places carry what the page said", () => {
    const jobs = board(jobsScene());
    // A short answer reads on from the lead, above the roles it introduces.
    expect(jobs.more?.blocks).toHaveLength(1);
    expect(jobs.blocks.some((block) => block.kind === "prose")).toBe(false);
    const roles = jobs.blocks.find((block) => block.kind === "gallery");
    expect(roles?.kind === "gallery" && roles.facet).toBe("job");
    expect(roles?.kind === "gallery" && roles.entities).toHaveLength(8);
    const rust = board(rustScene());
    expect(rust.blocks.map((block) => block.kind)).toEqual(["prose", "code", "table"]);
    expect(rust.blocks[0]!.emphasis).toBe("hero");
    const dinner = board(dinnerScene());
    const places = dinner.blocks.find((block) => block.kind === "gallery");
    if (places?.kind !== "gallery") throw new Error("places");
    expect(places.facet).toBe("place");
    expect(places.entities[0]!.facts.at(-1)).toMatchObject({
      label: "",
      value: "Roast chicken for two, book a week ahead",
    });
  });

  test("while a run is live and no answer stands, the prose waits with its label", () => {
    const scene = runningScene();
    const projection = [...scene.objectives.values()][0]!;
    const made = boardOf({
      id: "board",
      executions: projection.executions,
      elements: scene.snapshot.elements,
      pending: "The answer is on its way",
    });
    expect(made.blocks).toEqual([
      expect.objectContaining({
        kind: "prose",
        state: "pending",
        pending: "The answer is on its way",
      }),
    ]);
  });
});

describe("the process column", () => {
  test("the trail tells what the runs did in closed facts, and the step going on now", () => {
    const trip = [...tripScene().objectives.values()][0]!.executions;
    expect(runTrail(trip, false).map((line) => [line.icon, line.text])).toEqual([
      ["search", "Searched the web 3 times"],
      ["page", "Read 5 pages"],
      ["page", "1 page would not open"],
      ["time", "1m 47s on pages"],
    ]);
    const live = [...runningScene().objectives.values()][0]!.executions;
    expect(runTrail(live, true).at(-1)).toMatchObject({ live: true, text: "Reading airbnb.com" });
    const rust = [...rustScene().objectives.values()][0]!.executions;
    expect(runTrail(rust, false)).toEqual([
      expect.objectContaining({ icon: "knowledge", detail: "Nothing read for this" }),
    ]);
  });

  test("a run reads left to right on one spine: the request, a row per site, then the result", () => {
    const scene = runningScene();
    const [stage] = environmentStages(scene.snapshot, scene.objectives, {
      recorded: () => scene.pages,
    });
    const rects = stage!.lane.rects;
    expect(rects[stage!.card]).toMatchObject({ x: 0, y: 0, width: 320 });
    expect(stage!.parts.length).toBeGreaterThan(0);
    const browsing = stage!.parts.filter((part) => part.helper === "browser");
    for (const part of browsing) {
      expect(rects[part.id]!.x).toBe(320 + 48);
      expect(new Set(part.pages.map((page) => siteKey(new URL(page.url).host)))).toEqual(
        new Set([part.key]),
      );
    }
    // Rows stack 32 apart, the first on the request's spine.
    const tops = stage!.parts.map((part) => rects[part.id]!.y);
    expect(tops[0]! + 12).toBe(32);
    for (let index = 1; index < tops.length; index++)
      expect(tops[index]!).toBeGreaterThanOrEqual(
        tops[index - 1]! + rects[stage!.parts[index - 1]!.id]!.height + 32,
      );
    expect(stage!.lane.board.x % 8).toBe(0);
    for (const part of stage!.parts)
      expect(stage!.lane.board.x).toBeGreaterThanOrEqual(
        rects[part.id]!.x + rects[part.id]!.width + 48,
      );
    // One line per part, never one per page.
    expect(stage!.lane.lines.filter((line) => line.kind === "part")).toHaveLength(
      stage!.parts.length,
    );
    const done = tripScene();
    const [settled] = environmentStages(done.snapshot, done.objectives, {
      recorded: () => done.pages,
    });
    expect(settled!.parts.every((part) => part.state !== "running")).toBe(true);
    const feeds = settled!.lane.lines.filter((line) => line.kind === "feed");
    expect(feeds.length).toBeGreaterThan(0);
    expect(new Set(feeds.map((line) => line.target)).size).toBe(1);
  });

  test("a block the person dragged is kept where they put it, from its lane's corner; the rest stay in the board's flow", () => {
    const scene = saasScene();
    const stages = environmentStages(scene.snapshot, scene.objectives);
    const block = stages[0]!.board.blocks.find((entry) => entry.kind === "checklist")!;
    const target = stages[0]!.targets[block.id]!;
    const view = {
      positions: { ...stages[0]!.targets, [block.id]: { x: target.x + 200, y: target.y + 40 } },
      viewport: { x: 0, y: 0, zoom: 1 },
    };
    const saved = viewPlacements(scene.snapshot, view, stages, new Set([block.id]));
    const pin = saved.find((place) => place.element === block.id)!;
    const corner = stages[0]!.lane.corner;
    expect(pin).toMatchObject({
      revision: 3,
      x: target.x + 200 - corner.x,
      y: target.y + 40 - corner.y,
    });
    const pinned = environmentStages(
      { ...scene.snapshot, view: { ...scene.snapshot.view, placements: saved } },
      scene.objectives,
    );
    expect(pinned[0]!.targets[block.id]).toEqual({ x: target.x + 200, y: target.y + 40 });
    expect(pinned[0]!.pinned.has(block.id)).toBe(true);
    // Without a drag nothing is pinned, wherever the canvas says a block stands.
    const kept = viewPlacements(scene.snapshot, view, stages);
    expect(kept.find((place) => place.element === block.id)?.revision).toBeUndefined();
  });
});

describe("the architecture diagram", () => {
  test("thirteen parts stand in five tiers, one column each, at full size within a 1600 board", () => {
    const made = board(saasScene());
    const diagram = made.blocks.find((block) => block.kind === "diagram");
    if (diagram?.kind !== "diagram") throw new Error("diagram");
    const layout = diagramLayout(diagram.diagram);
    const xs = new Map<string, Set<number>>();
    for (const node of diagram.diagram.nodes) {
      const set = xs.get(node.layer ?? "") ?? new Set<number>();
      set.add(layout.at[node.id]!.x);
      xs.set(node.layer ?? "", set);
    }
    expect([...xs.values()].map((set) => set.size)).toEqual([1, 1, 1, 1, 1]);
    expect(layout.bounds.width + 2 * 20).toBeLessThanOrEqual(1600);
    const { width: W, height: H } = DIAGRAM.node;
    const parts = Object.values(layout.at);
    for (const [index, a] of parts.entries())
      for (const b of parts.slice(index + 1))
        expect(a.x + W <= b.x || b.x + W <= a.x || a.y + H <= b.y || b.y + H <= a.y).toBe(true);
    // No line runs through a part it does not start or end at.
    for (const [key, flow] of Object.entries(layout.flows)) {
      const points = flow.points;
      for (const [id, at] of Object.entries(layout.at)) {
        if (id === flow.from || id === flow.to) continue;
        points.slice(1).forEach((point, i) => {
          const from = points[i]!;
          const [lx, hx] = [Math.min(from.x, point.x), Math.max(from.x, point.x)];
          const [ly, hy] = [Math.min(from.y, point.y), Math.max(from.y, point.y)];
          const crosses = lx < at.x + W - 1 && hx > at.x + 1 && ly < at.y + H - 1 && hy > at.y + 1;
          expect(crosses, `${key} through ${id}`).toBe(false);
        });
      }
    }
  });
});
