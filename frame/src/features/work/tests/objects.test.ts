import { expect, test } from "vitest";
import { inline } from "../components/objects/inline";
import { blocks, noteBlocks } from "../components/objects/markdown";
import { bests, cellOrder, fitColumns, figure, rating, yesNo } from "../components/objects/sheet";
import { size } from "../components/objects/size-text";
import type { SheetColumn } from "../lib/board/types";

test("inline text reads only strong and code as marks", () => {
  expect(inline("A **bold** move with `code` and *not* this")).toEqual([
    { text: "A " },
    { text: "bold", strong: true },
    { text: " move with " },
    { text: "code", code: true },
    { text: " and *not* this" },
  ]);
});

test("a draft's Markdown is paragraphs and lists; a note's has headings", () => {
  expect(blocks("Hi Sarah,\n\n- one\n- two\n\nBye\nAlex")).toEqual([
    { kind: "paragraph", lines: ["Hi Sarah,"] },
    { kind: "list", items: ["one", "two"] },
    { kind: "paragraph", lines: ["Bye", "Alex"] },
  ]);
  expect(noteBlocks("# Pricing\n\n## Open\n- a\n- b")).toEqual([
    { kind: "heading", text: "Pricing" },
    { kind: "heading", text: "Open" },
    { kind: "list", items: ["a", "b"] },
  ]);
});

test("sheet cells compare by their type, and the best value is marked only where values differ", () => {
  const money: SheetColumn = { label: "Price", kind: "money", currency: "USD", best: "min" };
  const score: SheetColumn = { label: "Rating", kind: "rating", best: "max" };
  expect(cellOrder(money, "1,142")).toBe(1142);
  expect(cellOrder(score, "4/5")).toBe(0.8);
  expect(cellOrder({ label: "Bag", kind: "yes_no" }, "partial")).toBe(1);
  expect(rating("11/10")).toBeNull();
  expect(yesNo("Maybe")).toBe("unknown");
  expect(figure(money, "20")).toBe("$20");
  const marked = bests({
    columns: [{ label: "Name", kind: "text" }, money, score],
    rows: [
      { cells: ["A", "20", "4/5"] },
      { cells: ["B", "5", "4/5"] },
      { cells: ["C", "5", "3/5"] },
    ],
  });
  expect([...marked[1]!]).toEqual(["1", "2"]);
  expect([...marked[2]!]).toEqual(["0", "1"]);
  const same = bests({ columns: [money], rows: [{ cells: ["5"] }, { cells: ["5"] }] });
  expect(same[0]!.size).toBe(0);
});

test("a file's size reads as the system writes it", () => {
  expect(size(412_000)).toBe("412 KB");
  expect(size(3_200)).toBe("3.2 KB");
  expect(size(18_400_000)).toBe("18 MB");
});

test("a sheet keeps its subject and the columns that say most in the width it has", () => {
  const costs = {
    columns: [
      { label: "Cost category", kind: "text" as const },
      { label: "MVP per month", kind: "money" as const },
      { label: "Growth per month", kind: "money" as const },
    ],
    rows: [
      ["Application hosting and edge", "$100–800", "$1,000–8,000"],
      ["Queue, workers and background compute", "$100–1,000", "$1,000–15,000"],
    ].map((cells) => ({ cells })),
  };
  expect(fitColumns(costs, 360)).toEqual([1]);
  expect(fitColumns(costs, 560)).toEqual([1, 2]);
  const stack = {
    columns: [
      { label: "Layer", kind: "text" as const },
      { label: "Technology", kind: "text" as const },
      { label: "Why", kind: "text" as const },
      { label: "Managed", kind: "yes_no" as const },
    ],
    rows: [
      [
        "Web client",
        "Next.js + TypeScript",
        "Fast product iteration, typed UI and server-rendered pages.",
        "yes",
      ],
      [
        "API",
        "TypeScript service (Fastify or NestJS)",
        "Stateless endpoints, validation, tenant authorization and billing hooks.",
        "no",
      ],
    ].map((cells) => ({ cells })),
  };
  // The check first, then the shorter text; the sentences give way until there is room.
  expect(fitColumns(stack, 420)).toEqual([1, 3]);
  expect(fitColumns(stack, 900)).toEqual([1, 2, 3]);
});
