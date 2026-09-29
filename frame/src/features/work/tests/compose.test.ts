import { expect, test } from "vitest";
import type { WorkExecutionFact } from "$shared/ipc/bindings";
import { freeSpot } from "../lib/free-space";
import { provenance } from "../lib/run/provenance";
import { pageName } from "../lib/run/sources";
import { versus } from "../lib/board/versus";
import { pickOf } from "../components/objects/sheet";
import type { PicksView, SheetView } from "../lib/board/types";
import type { PageGroup } from "../lib/project-environment-stage";

const step = (id: string, url: string, artifacts: string[] = [], title?: string) =>
  ({
    id,
    kind: { kind: "read", url },
    status: "succeeded",
    artifacts,
    ...(title ? { local: { page_title: title } } : {}),
  }) as unknown as NonNullable<WorkExecutionFact["steps"]>[number];

const run = {
  id: "run",
  artifacts: [
    { id: "a1", evidence: [{ extraction_id: "x1", source_id: 1 }], data: { kind: "findings" } },
    { id: "a2", evidence: [{ extraction_id: "x2", source_id: 1 }], data: { kind: "findings" } },
  ],
  steps: [
    step("s0", "https://www.lego.com/en-us/themes/architecture"),
    step("s1", "https://www.lego.com/en-us/product/paris-21064", ["a1"]),
    step("s2", "https://www.lego.com/en-us/product/tower-bridge-21067", ["a2"]),
  ],
  provider_evidence: [],
} as unknown as WorkExecutionFact;

const group = (id: string, url: string, steps: string[]): PageGroup =>
  ({ id, url, steps: steps.map((each) => ({ id: each })) }) as unknown as PageGroup;
const pages = [
  group("theme", "https://www.lego.com/en-us/themes/architecture", ["s0"]),
  group("paris", "https://www.lego.com/en-us/product/paris-21064", ["s1"]),
  group("tower", "https://www.lego.com/en-us/product/tower-bridge-21067", ["s2"]),
];
const pick = (name: string, extra: Partial<PicksView["items"][number]> = {}) => ({
  name,
  facts: [],
  tags: [],
  recommended: false,
  ...extra,
});

test("what a part found stands under the page it was taken from, the index page left in its stack", () => {
  const picks: PicksView = {
    id: "p",
    kind: "picks",
    facet: "product",
    sources: {},
    items: [
      pick("Paris", { url: "https://www.lego.com/en-us/product/paris-21064/" }),
      pick("Tower Bridge", { sources: ["x2:1"] }),
    ],
  };
  const told = provenance(picks, pages, [run], [])!;
  expect(told.view.items.map((item) => item.from?.page)).toEqual(["paris", "tower"]);
  expect([...told.used].sort()).toEqual(["paris", "tower"]);
  expect(told.view.items[1]!.from!.title).toBe("Tower bridge");
});

test("things all taken from one page say nothing of where they came from", () => {
  const picks: PicksView = {
    id: "p",
    kind: "picks",
    facet: "product",
    sources: {},
    items: [pick("A", { sources: ["x1:1"] }), pick("B", { sources: ["x1:1"] })],
  };
  expect(provenance(picks, pages, [run], [])).toBeNull();
});

test("a page is named by its title, a thing taken from it, or its address, never by its host", () => {
  expect(pageName([run], "https://www.lego.com/en-us/product/paris-21064")).toBe("Paris");
  expect(pageName([run], "https://example.com/search?q=lego+architecture")).toBe(
    "lego architecture",
  );
  expect(pageName([run], "https://github.com/acme/app/issues/123")).toBe("Issues 123");
  expect(pageName([run], "https://example.com/")).toBe("");
});

test("a note lands in the nearest room that touches nothing", () => {
  const size = { width: 200, height: 100 };
  expect(freeSpot({ x: 500, y: 500 }, size, [])).toEqual({ x: 500, y: 498 });
  const taken = [{ x: 300, y: 400, width: 400, height: 200 }];
  const spot = freeSpot({ x: 500, y: 500 }, size, taken);
  const rect = { x: spot.x - 100, y: spot.y - 50, ...size };
  const [block] = taken;
  expect(
    rect.x + rect.width + 32 <= block!.x ||
      block!.x + block!.width + 32 <= rect.x ||
      rect.y + rect.height + 32 <= block!.y ||
      block!.y + block!.height + 32 <= rect.y,
  ).toBe(true);
  expect(Math.hypot(spot.x - 500, spot.y - 500)).toBeLessThan(260);
});

test("a few named things compared stand as columns, and the pick is the one best most often", () => {
  const sheet: SheetView = {
    id: "s",
    kind: "sheet",
    columns: [
      { label: "", kind: "entity" },
      { label: "Price", kind: "money", best: "min" },
      { label: "Pieces", kind: "number", best: "max" },
      { label: "Theme", kind: "text" },
    ],
    rows: [
      { cells: ["Paris", "79.99", "958", "City"] },
      { cells: ["Tower Bridge", "349.99", "3745", "City"] },
      { cells: ["New York", "59.99", "1465", "City"] },
    ],
  };
  expect(versus(sheet)).toBe(true);
  expect(pickOf(sheet)).toBeNull();
  const lighter = {
    ...sheet,
    columns: sheet.columns.map((column, at) =>
      at === 2 ? { ...column, best: "min" as const } : column,
    ),
    rows: [...sheet.rows.slice(0, 2), { cells: ["New York", "59.99", "900", "City"] }],
  };
  expect(pickOf(lighter)).toBe(2);
  expect(versus({ ...sheet, rows: [...sheet.rows, ...sheet.rows] })).toBe(false);
});
