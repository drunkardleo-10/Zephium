import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { loadChart } from "../index";
test("renders SVG on demand and retains exact values when a plot is unavailable", async () => {
  const { default: Chart } = await loadChart();
  const screen = await render(Chart, {
    title: "Measurements",
    xLabel: "Sample",
    yLabel: "Time",
    series: [
      {
        name: "Baseline",
        points: [
          { label: "One", value: "2.5000" },
          { label: "Two", value: "3.75" },
        ],
      },
    ],
  });
  await expect.poll(() => screen.container.querySelector("svg")).not.toBeNull();
  await screen.getByText("Exact values", { exact: true }).click();
  await expect.element(screen.getByRole("cell", { name: "2.5000" })).toBeVisible();
  await screen.rerender({
    series: [{ name: "Unsupported", points: [{ label: "One", value: "9007199254740993" }] }],
  });
  expect(screen.container.querySelector("svg")).toBeNull();
  expect(screen.container.textContent).toContain("9007199254740993");
});
