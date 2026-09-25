import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import WorkCanvas from "../components/WorkCanvas.svelte";
import type { CanvasItem } from "../lib/canvas-model";
test("explicit result focus restores readable zoom without moving nodes and supports keyboard reading", async () => {
  await page.viewport(1100, 750);
  const paragraphs = Array.from(
    { length: 30 },
    (_, i) => `Research paragraph ${i + 1}: preserved source text.`,
  );
  const items: CanvasItem[] = [
    {
      id: "result",
      title: "Research",
      kind: "Result",
      detail: "",
      status: "Review required",
      layout: "artifact",
      artifact: {
        key: "artifact",
        title: "Research",
        reviewLabel: "Review required",
        evidence: [{ key: "evidence:1", label: "Source 1" }],
        content: { kind: "document", paragraphs },
      },
    },
  ];
  const onviewchange = vi.fn();
  const oninspect = vi.fn();
  const onevidence = vi.fn();
  const onaction = vi.fn();
  const screen = await render(WorkCanvas, {
    items,
    links: [],
    authoritative: new Set(["result"]),
    onaction,
    initialView: {
      positions: { result: { x: 800, y: 400 } },
      sizes: { result: { width: 480, height: 360 } },
      viewport: { x: -100, y: -60, zoom: 0.4 },
    },
    fitBottomInset: 100,
    onviewchange,
    oninspect,
    onevidence,
  });
  screen.container.style.width = "1100px";
  screen.container.style.height = "750px";
  await screen.getByText("Research", { exact: true }).click();
  await screen.getByRole("button", { name: "Focus result", exact: true }).click();
  await expect.poll(() => onviewchange.mock.lastCall?.[0].viewport.zoom).toBe(1);
  expect(onviewchange.mock.lastCall?.[0].positions.result).toEqual({ x: 800, y: 400 });
  expect(onviewchange.mock.lastCall?.[0].sizes.result).toEqual({ width: 480, height: 360 });
  const body = screen.getByRole("button", { name: "Research", exact: true });
  await expect.element(body).toHaveFocus();
  // The card reads as the answer: its lead, not the whole text, and no citation chips.
  const card = screen.container.querySelector(".artifact-body")!;
  expect(card.textContent).toContain(paragraphs[0]);
  expect(card.textContent).not.toContain(paragraphs.at(-1));
  expect(card.querySelector(".chip")).toBeNull();
  oninspect.mockClear();
  await screen.getByText(paragraphs[0]!, { exact: true }).click();
  expect(oninspect).not.toHaveBeenCalled();
  expect(onevidence).not.toHaveBeenCalled();
  await expect
    .element(screen.getByRole("button", { name: "Inspect", exact: true }))
    .not.toBeInTheDocument();
  expect(onaction).not.toHaveBeenCalled();
  await screen.unmount();
});

test("a nine-column comparison is a mini table of four columns under one visible canvas title", async () => {
  await page.viewport(1100, 750);
  const title =
    "A detailed comparison of nine research criteria across the shortlisted alternatives and their documented limitations";
  const criteria = [
    "Purchase price",
    "Regional availability",
    "Mechanical construction",
    "Connection options",
    "Warranty coverage",
    "Repairability",
    "Documented limitations",
    "Source confidence",
    "Delivery conditions",
  ];
  const item: CanvasItem = {
    id: "comparison",
    title,
    kind: "Result",
    detail: "",
    status: "Review required",
    layout: "artifact",
    artifact: {
      key: "comparison",
      title,
      reviewLabel: "Review required",
      evidence: [],
      content: {
        kind: "comparison",
        criteria,
        alternatives: [
          {
            name: "First alternative",
            values: criteria.map((criterion) => `Evidence for ${criterion}`),
          },
          {
            name: "Second alternative",
            values: criteria.map((criterion) => `Further evidence for ${criterion}`),
          },
        ],
      },
    },
  };
  const screen = await render(WorkCanvas, {
    items: [item],
    links: [],
    authoritative: new Set(["comparison"]),
    initialView: {
      positions: { comparison: { x: 100, y: 100 } },
      viewport: { x: 0, y: 0, zoom: 0.5 },
    },
    oninspect: vi.fn(),
    fitBottomInset: 100,
  });
  screen.container.style.width = "1100px";
  screen.container.style.height = "750px";
  await screen.container.querySelector<HTMLElement>(".work-drag-handle")!.click();
  await screen.getByRole("button", { name: "Focus result", exact: true }).click();
  // The card is the table's opening: the names and the first three criteria; the lift has the rest.
  const table = screen.container.querySelector<HTMLTableElement>(".mini")!;
  await expect.element(table).toBeVisible();
  expect([...table.querySelectorAll("thead th")].map((cell) => cell.textContent)).toEqual([
    "",
    "Purchase price",
    "Regional availability",
    "Mechanical construction",
  ]);
  expect(table.textContent).toContain("Second alternative");
  expect(table.textContent).not.toContain("Delivery conditions");
  expect(screen.container.querySelector(".artifact h2")).toBeNull();
  expect(screen.getByText(title, { exact: true }).elements()).toHaveLength(1);
  const viewport = screen.container.querySelector<HTMLElement>(".svelte-flow__viewport")!;
  const transform = viewport.style.transform;
  await screen.getByText("Purchase price", { exact: true }).click();
  expect(viewport.style.transform).toBe(transform);
  await screen.unmount();
});
