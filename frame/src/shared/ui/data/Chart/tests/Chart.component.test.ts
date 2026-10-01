import { afterEach, expect, test, vi } from "vitest";
import { page } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import "$styles/global.css";
import { loadChart } from "../index";
import type { ChartKind, ChartSpec } from "../chart";

afterEach(() => {
  document.documentElement.dataset.reduceMotion = "false";
});

const two = (kind: ChartKind, extra: Partial<ChartSpec> = {}): ChartSpec => ({
  kind,
  x: { label: "Day" },
  y: { label: "Minutes" },
  series: [
    {
      name: "Reading",
      points: [
        { x: "Mon", y: 30 },
        { x: "Tue", y: 10 },
        { x: "Wed", y: 20 },
      ],
    },
    {
      name: "Writing",
      points: [
        { x: "Mon", y: 5 },
        { x: "Tue", y: 15 },
        { x: "Wed", y: 25 },
      ],
    },
  ],
  ...extra,
});

const texts = (root: Element, selector: string) =>
  [...root.querySelectorAll(selector)].map((node) => node.textContent?.trim() ?? "");

/** LayerChart's band hit area for one category, entered the way a pointer would. */
function hover(root: Element, index: number) {
  const hit = root.querySelectorAll<SVGRectElement>(".lc-tooltip-rect")[index]!;
  const box = hit.getBoundingClientRect();
  const at = { clientX: box.x + box.width / 2, clientY: box.y + box.height / 2, bubbles: true };
  hit.dispatchEvent(new PointerEvent("pointerenter", at));
  hit.dispatchEvent(new PointerEvent("pointermove", at));
}

test.each([
  ["bars", ".mark.bar", 6],
  ["stacked", ".mark.bar", 6],
  ["range", ".mark.bar", 6],
  ["line", "path.mark.line", 2],
  ["area", "path.mark.area", 2],
  ["donut", "path.mark.slice", 3],
  ["radial", "path.mark.slice", 3],
  ["radar", "polygon.mark.web", 2],
  ["heat", "rect.mark.cell", 6],
  ["spark", "path.mark.line", 1],
] as const)("%s draws its marks through LayerChart", async (kind, selector, count) => {
  const { default: Chart } = await loadChart();
  const spec = two(kind);
  const screen = await render(Chart, {
    title: "Week",
    height: 24,
    spec:
      kind === "range"
        ? {
            ...spec,
            series: spec.series.map((series) => ({
              ...series,
              points: series.points.map((point) => ({ ...point, y2: point.y! + 5 })),
            })),
          }
        : spec,
  });
  await expect.poll(() => screen.container.querySelectorAll(selector).length).toBe(count);
  expect(screen.container.querySelector("canvas")).toBeNull();
  await expect
    .element(screen.getByRole("img", { name: /^Week\. Day; Minutes\./u }))
    .toBeInTheDocument();
});

test("the lift names the extremes to assistive technology, keeps a legend and copies its values", async () => {
  const { default: Chart } = await loadChart();
  const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  const screen = await render(Chart, { title: "Week", spec: two("bars") });
  await expect
    .element(
      screen.getByRole("img", {
        name: "Week. Day; Minutes. Bar chart; lowest Mon at 5, highest Mon at 30",
      }),
    )
    .toBeInTheDocument();
  // The extremes are read aloud, never written under the plot.
  expect(screen.container.textContent).not.toContain("lowest Mon at 5");
  await expect.element(screen.getByRole("list").getByText("Reading")).toBeVisible();
  expect(texts(screen.container, ".lc-axis-tick-label")).toEqual(
    expect.arrayContaining(["0", "30", "Mon", "Wed"]),
  );
  await screen.getByText("Values", { exact: true }).click();
  await screen.getByRole("button", { name: "Copy values (CSV)" }).click();
  expect(writeText).toHaveBeenCalledWith("Day,Reading,Writing\nMon,30,5\nTue,10,15\nWed,20,25");
});

test("the lift shows every series at the hovered x, and hands a source back", async () => {
  await page.viewport(800, 600);
  const { default: Chart } = await loadChart();
  const onevidence = vi.fn();
  const reference = { key: "a", label: "Bench", origin: "bench.io" };
  const screen = await render(Chart, {
    title: "Measurements",
    spec: {
      kind: "bars",
      x: { label: "Sample" },
      y: { label: "Time" },
      basis: "Basis: Same rig",
      knowledge: true,
      series: [
        {
          name: "Baseline",
          points: [
            { x: "One", y: 2.5, display: "2.5000", evidence: [reference] },
            { x: "Two", y: 3.75 },
          ],
        },
        {
          name: "Tuned",
          points: [
            { x: "One", y: 1.5 },
            { x: "Two", y: 2 },
          ],
        },
      ],
    },
    onevidence,
  });
  await expect.element(screen.getByText("Basis: Same rig", { exact: true })).toBeVisible();
  // Known numbers keep their basis line and draw no disclaimer.
  expect(screen.container.textContent).not.toContain("agent knows");
  await expect.poll(() => screen.container.querySelectorAll(".lc-tooltip-rect").length).toBe(2);
  expect(screen.container.querySelector(".chart-tooltip")).toBeNull();
  hover(screen.container, 0);
  await expect.poll(() => screen.container.querySelector(".chart-tooltip")).not.toBeNull();
  const tip = screen.container.querySelector<HTMLElement>(".chart-tooltip")!;
  expect(tip.textContent).toContain("One");
  expect(tip.textContent).toContain("Baseline");
  expect(tip.textContent).toContain("2.5000");
  expect(tip.textContent).toContain("Tuned");
  expect(tip.textContent).toContain("1.5");
  tip.querySelector<HTMLButtonElement>(".chip")!.click();
  expect(onevidence).toHaveBeenCalledWith(reference);
  // The exact value stays in the table, with the precision note.
  await screen.getByText("Exact values", { exact: true }).click();
  await expect.element(screen.getByRole("cell", { name: "2.5000" })).toBeVisible();
  await expect.element(screen.getByText(/The plot is approximate/u)).toBeVisible();
});

test("a bars card writes its categories under the bars and each value on top", async () => {
  const { default: Chart } = await loadChart();
  const screen = await render(Chart, {
    title: "Quotes",
    spec: {
      kind: "bars",
      compact: true,
      y: { format: "money", currency: "USD" },
      series: [
        {
          name: "Quote",
          points: [
            { x: "Acme", y: 3400 },
            { x: "Bolt", y: 1200 },
            { x: "Core", y: 800 },
          ],
        },
      ],
    },
  });
  await expect.poll(() => screen.container.querySelectorAll(".mark.bar").length).toBe(3);
  expect(texts(screen.container, ".lc-axis-tick-label")).toEqual(["Acme", "Bolt", "Core"]);
  expect(texts(screen.container, "text.value")).toEqual(["$3.4K", "$1.2K", "$800"]);
  // One series names itself; the card keeps no legend and no table.
  expect(screen.container.querySelector(".chart-legend")).toBeNull();
  expect(screen.container.querySelector("details")).toBeNull();
});

test("a line card reads its first and last x and its last value", async () => {
  const { default: Chart } = await loadChart();
  const screen = await render(Chart, {
    title: "Trend",
    spec: {
      kind: "line",
      compact: true,
      y: { format: "percent" },
      series: [
        {
          name: "Share",
          points: ["Jan", "Feb", "Mar", "Apr"].map((x, index) => ({ x, y: 30 + index * 5 })),
        },
      ],
    },
  });
  await expect.poll(() => screen.container.querySelectorAll("path.mark.line").length).toBe(1);
  expect(texts(screen.container, ".lc-axis-tick-label")).toEqual(["Jan", "Apr"]);
  expect(texts(screen.container, "text.value")).toEqual(["45%"]);
});

test("a donut card states its largest slice and its share", async () => {
  const { default: Chart } = await loadChart();
  const screen = await render(Chart, {
    title: "Split",
    spec: {
      kind: "donut",
      compact: true,
      series: [
        {
          name: "Time",
          points: [
            { x: "Mail", y: 20 },
            { x: "Docs", y: 60 },
            { x: "Chat", y: 20 },
          ],
        },
      ],
    },
  });
  await expect.poll(() => screen.container.querySelectorAll("path.mark.slice").length).toBe(3);
  expect(screen.container.querySelector(".total")?.textContent).toBe("60%");
  expect(screen.container.querySelector(".lead")?.textContent).toBe("Docs");
});

test("a card with two series keeps a two-row legend; past four, the values table", async () => {
  const { default: Chart } = await loadChart();
  const pair = await render(Chart, { title: "Week", spec: two("bars", { compact: true }) });
  await expect.poll(() => pair.container.querySelectorAll(".chart-legend.rows li").length).toBe(2);
  expect(pair.container.querySelector("details")).toBeNull();
  await pair.unmount();
  const six: ChartSpec = {
    kind: "line",
    compact: true,
    series: Array.from({ length: 6 }, (_, order) => ({
      name: `S${order + 1}`,
      points: [
        { x: "Mon", y: order },
        { x: "Tue", y: order + 1 },
      ],
    })),
  };
  const many = await render(Chart, { title: "Six", spec: six });
  await expect.poll(() => many.container.querySelectorAll("path.mark.line").length).toBe(6);
  expect(many.container.querySelector(".chart-legend")).toBeNull();
  await many.getByText("Values", { exact: true }).click();
  await expect.element(many.getByRole("columnheader", { name: "S6" })).toBeVisible();
  expect(many.container.querySelectorAll("tbody tr")).toHaveLength(2);
});

test("a chart arrives once, and reduced motion draws it settled", async () => {
  const { default: Chart } = await loadChart();
  const moving = await render(Chart, { title: "Week", spec: two("area") });
  await expect.poll(() => moving.container.querySelectorAll("path.area.enter").length).toBe(2);
  // The lines draw in through LayerChart once; the washes fade in beside them.
  const line = moving.container.querySelector("path.mark.line")!;
  expect(line.getAnimations().length).toBeGreaterThan(0);
  await expect
    .poll(() => moving.container.querySelectorAll(".enter").length, { timeout: 2000 })
    .toBe(0);
  await moving.unmount();
  document.documentElement.dataset.reduceMotion = "true";
  const still = await render(Chart, { title: "Week", spec: two("area") });
  await expect.poll(() => still.container.querySelectorAll("path.mark.area").length).toBe(2);
  expect(still.container.querySelectorAll(".enter")).toHaveLength(0);
  for (const path of still.container.querySelectorAll("path.mark"))
    expect(path.getAnimations()).toHaveLength(0);
});

test("nothing to plot says so and keeps the values", async () => {
  const { default: Chart } = await loadChart();
  const screen = await render(Chart, {
    title: "Empty",
    spec: {
      kind: "bars",
      series: [{ name: "S", points: [{ x: "A", y: null, display: "unknown" }] }],
    },
  });
  await expect.element(screen.getByRole("status")).toHaveTextContent("No values to plot.");
  expect(screen.container.textContent).toContain("unknown");
});
