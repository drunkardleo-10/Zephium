import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { loadChart } from "../index";
test("plots bars from exact values, states its basis, and hands a point's sources back", async () => {
  const { default: Chart } = await loadChart();
  const onevidence = vi.fn();
  const screen = await render(Chart, {
    title: "Measurements",
    xLabel: "Sample",
    yLabel: "Time",
    basis: { method: "Same rig", observedAt: "2026-09-16" },
    series: [
      {
        name: "Baseline",
        points: [
          {
            label: "One",
            value: "2.5000",
            evidence: [{ key: "a", label: "Bench", origin: "b.io" }],
          },
          { label: "Two", value: "3.75" },
        ],
      },
    ],
    onevidence,
  });
  await expect.poll(() => screen.container.querySelectorAll("rect.bar").length).toBe(2);
  const bars = [...screen.container.querySelectorAll("rect.bar")].map((bar) =>
    Number(bar.getAttribute("height")),
  );
  expect(bars[0]).toBeGreaterThan(0);
  expect(bars[1]).toBeGreaterThan(bars[0]!);
  await expect
    .element(screen.getByText("Basis: Same rig · 2026-09-16", { exact: true }))
    .toBeVisible();
  await screen.getByText("Exact values", { exact: true }).click();
  await expect.element(screen.getByRole("cell", { name: "2.5000" })).toBeVisible();
  await screen.getByRole("button", { name: "b.io", exact: true }).first().click();
  expect(onevidence).toHaveBeenCalledWith({ key: "a", label: "Bench", origin: "b.io" });
  // A value the plot cannot place keeps its exact decimal instead of a bar.
  await screen.rerender({
    series: [{ name: "Unsupported", points: [{ label: "One", value: "9007199254740993" }] }],
  });
  expect(screen.container.querySelector("rect.bar")).toBeNull();
  expect(screen.container.textContent).toContain("9007199254740993");
});

test("several series read as lines with a legend", async () => {
  const { default: Chart } = await loadChart();
  const screen = await render(Chart, {
    title: "Prices",
    xLabel: "Model",
    yLabel: "USD",
    series: [
      {
        name: "List",
        points: [
          { label: "A", value: "10" },
          { label: "B", value: "20" },
        ],
      },
      {
        name: "Street",
        points: [
          { label: "A", value: "8" },
          { label: "B", value: "16" },
        ],
      },
    ],
  });
  await expect.poll(() => screen.container.querySelectorAll("polyline.line").length).toBe(2);
  await expect.element(screen.getByText("Street", { exact: true }).first()).toBeVisible();
  expect(screen.container.querySelector("rect.bar")).toBeNull();
});
