import { describe, expect, test } from "vitest";
import { boardLayout, BOARD, type LayoutBlock } from "../lib/board/layout";
import { boardOf } from "../lib/board/adapter";
import { runTrail } from "../lib/board/trail";
import { environmentStages } from "../lib/project-environment-board";
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
      expect(layout.width).toBeLessThanOrEqual(BOARD.max);
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
  test("an architecture: the answer's first sentence is the lead, the diagram leads, the cost table and chart pair", () => {
    const made = board(saasScene());
    expect(made.title).toBe("How to build a B2B SaaS that scales");
    expect(made.lead).toMatch(/^Start as a modular monolith .* file processing\.$/u);
    const kinds = made.blocks.map((block) => [block.kind, block.emphasis, block.title ?? ""]);
    expect(kinds).toContainEqual(["diagram", "hero", "Reference architecture"]);
    expect(kinds).toContainEqual(["prose", "primary", "How to build a B2B SaaS that scales"]);
    expect(kinds).toContainEqual(["checklist", "supporting", "Launch checklist"]);
    const table = made.blocks.find(
      (block) => block.title === "Monthly cost by stage" && block.kind === "table",
    )!;
    const chart = made.blocks.find((block) => block.kind === "chart")!;
    expect(chart.group).toBe(table.group);
    expect(made.blocks.indexOf(chart)).toBe(made.blocks.indexOf(table) + 1);
    if (chart.kind !== "chart") throw new Error("chart");
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

  test("a lane stands its column at x = 0 and its board beside it; a live run shows its pages, a finished one folds them into its sources", () => {
    const scene = runningScene();
    const [stage] = environmentStages(scene.snapshot, scene.objectives, {
      recorded: () => scene.pages,
    });
    expect(stage!.column.pages).toHaveLength(2);
    const rects = stage!.lane.rects;
    expect(rects[stage!.card]).toMatchObject({ x: 0, y: 0, width: 300 });
    expect(rects[stage!.column.trail!]!.y).toBe(rects[stage!.card]!.height + 12);
    expect(stage!.lane.board.x).toBe(348);
    const done = tripScene();
    const [settled] = environmentStages(done.snapshot, done.objectives, {
      recorded: () => done.pages,
    });
    expect(settled!.column.pages).toEqual([]);
    expect(settled!.column.sources).toBe(`sources:${settled!.card}`);
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
    expect(pin).toMatchObject({ revision: 3, x: target.x + 200 - 348, y: target.y + 40 });
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
