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

test.each([
  ["bars", "path.bar", 6],
  ["stacked", "path.bar", 6],
  ["range", "path.bar", 6],
  ["line", "path.line", 2],
  ["area", "path.area", 2],
  ["donut", "path.slice", 3],
  ["heat", "rect.cell", 6],
  ["spark", "path.line", 2],
] as const)("%s draws its marks", async (kind, selector, count) => {
  const { default: Chart } = await loadChart();
  const spec = two(kind);
  const screen = await render(Chart, {
    title: "Week",
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
  await expect
    .element(screen.getByRole("img", { name: /^Week\. Day; Minutes\./u }))
    .toBeInTheDocument();
});

test("the accessible name states the kind and the extremes", async () => {
  const { default: Chart } = await loadChart();
  const screen = await render(Chart, { title: "Week", spec: two("bars") });
  await expect
    .element(
      screen.getByRole("img", {
        name: "Week. Day; Minutes. Bar chart; lowest Mon at 5, highest Mon at 30",
      }),
    )
    .toBeInTheDocument();
  await expect.element(screen.getByRole("list").getByText("Reading")).toBeVisible();
});

test("hover shows the point, its value and its sources, and hands a source back", async () => {
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
      series: [
        {
          name: "Baseline",
          points: [
            { x: "One", y: 2.5, display: "2.5000", evidence: [reference] },
            { x: "Two", y: 3.75 },
          ],
        },
      ],
    },
    onevidence,
  });
  await expect.element(screen.getByText("Basis: Same rig", { exact: true })).toBeVisible();
  expect(screen.container.querySelector(".tip")).toBeNull();
  const hit = screen.container.querySelector<SVGRectElement>("rect.hit")!;
  hit.dispatchEvent(new PointerEvent("pointerenter"));
  await expect.poll(() => screen.container.querySelector(".tip")).not.toBeNull();
  const tip = screen.container.querySelector<HTMLElement>(".tip")!;
  expect(tip.textContent).toContain("One");
  expect(tip.textContent).toContain("2.5000");
  tip.querySelector<HTMLButtonElement>(".chip")!.click();
  expect(onevidence).toHaveBeenCalledWith(reference);
  // The exact value stays in the table, with the precision note.
  await screen.getByText("Exact values", { exact: true }).click();
  await expect.element(screen.getByRole("cell", { name: "2.5000" })).toBeVisible();
  await expect.element(screen.getByText(/The plot is approximate/u)).toBeVisible();
});

test("a chart arrives once, and reduced motion draws it settled", async () => {
  const { default: Chart } = await loadChart();
  const moving = await render(Chart, { title: "Week", spec: two("line") });
  await expect.poll(() => moving.container.querySelectorAll("path.line.enter").length).toBe(2);
  await expect
    .poll(() => moving.container.querySelectorAll(".enter").length, { timeout: 2000 })
    .toBe(0);
  await moving.unmount();
  document.documentElement.dataset.reduceMotion = "true";
  const still = await render(Chart, { title: "Week", spec: two("bars") });
  await expect.poll(() => still.container.querySelectorAll("path.bar").length).toBe(6);
  expect(still.container.querySelectorAll(".enter")).toHaveLength(0);
  for (const bar of still.container.querySelectorAll("path.bar"))
    expect(getComputedStyle(bar).animationName).toBe("none");
});

test("compact hides the axes, the legend, the basis and the values", async () => {
  const { default: Chart } = await loadChart();
  const screen = await render(Chart, {
    title: "Week",
    spec: two("bars", { compact: true, basis: "Basis: Same rig" }),
  });
  await expect.poll(() => screen.container.querySelectorAll("path.bar").length).toBe(6);
  expect(screen.container.querySelectorAll("text.tick")).toHaveLength(0);
  expect(screen.container.querySelector(".legend")).toBeNull();
  expect(screen.container.querySelector("details")).toBeNull();
  expect(screen.container.querySelectorAll("line.grid").length).toBeGreaterThan(0);
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
