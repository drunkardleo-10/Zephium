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
  const body = screen.getByRole("region", { name: "Research", exact: true });
  await expect.element(body).toHaveFocus();
  const scroll = screen.container.querySelector(".artifact-body")!;
  expect(scroll.scrollHeight).toBeGreaterThan(scroll.clientHeight);
  expect(scroll.textContent).toContain(paragraphs.at(-1));
  oninspect.mockClear();
  await screen.getByText(paragraphs[0]!, { exact: true }).click();
  expect(oninspect).not.toHaveBeenCalled();
  await screen.getByRole("button", { name: "Source 1", exact: true }).click();
  expect(onevidence).toHaveBeenCalledExactlyOnceWith("result", {
    key: "evidence:1",
    label: "Source 1",
  });
  expect(oninspect).not.toHaveBeenCalled();
  await expect
    .element(screen.getByRole("button", { name: "Inspect", exact: true }))
    .not.toBeInTheDocument();
  expect(onaction).not.toHaveBeenCalled();
  await screen.unmount();
});

test("a nine-column comparison stays readable and scrollable with one visible canvas title", async () => {
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
  const table = screen.getByRole("table", { name: title, exact: true });
  await expect.element(table).toBeVisible();
  const scroll = screen.container.querySelector(".table-scroll")!;
  expect(scroll.scrollWidth).toBeGreaterThan(scroll.clientWidth * 2);
  for (const heading of screen.container.querySelectorAll("th"))
    expect(heading.getBoundingClientRect().width).toBeGreaterThanOrEqual(160);
  expect(screen.container.querySelector(".artifact h2")).toBeNull();
  expect(screen.container.querySelector("caption")?.classList.contains("sr-only")).toBe(true);
  const viewport = screen.container.querySelector<HTMLElement>(".svelte-flow__viewport")!;
  const transform = viewport.style.transform;
  await table.getByRole("columnheader", { name: "Delivery conditions", exact: true }).click();
  expect(viewport.style.transform).toBe(transform);
  expect(scroll.scrollLeft).toBeGreaterThan(0);
  expect(scroll.textContent).toContain("Further evidence for Delivery conditions");
  await screen.unmount();
});
