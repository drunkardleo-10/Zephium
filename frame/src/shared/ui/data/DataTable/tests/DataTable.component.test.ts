import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import DataTable from "../DataTable.svelte";
import type { TableLabels } from "../table";
const labels: TableLabels = {
  rowHeading: "Option",
  actions: "Actions",
  empty: "No options",
  missing: "Not provided",
  unavailable: "Data unavailable",
  previous: "Previous",
  next: "Next",
  range: (first, last, total) => `${first}–${last} of ${total}`,
};
test("paginates a bounded presentation without fabricating totals or rendering markup", async () => {
  const rows = Array.from({ length: 60 }, (_, i) => ({
    key: String(i),
    label: `Option ${i}`,
    cells: { price: i === 0 ? "<img src=x onerror=unsafe()>" : `${i}` },
  }));
  const screen = await render(DataTable, {
    caption: "Accommodation",
    columns: [{ key: "price", label: "Price" }],
    rows,
    labels,
  });
  expect(screen.container.querySelectorAll("tbody tr")).toHaveLength(25);
  expect(screen.container.querySelector("img")).toBeNull();
  await expect.element(screen.getByRole("status")).toHaveTextContent("1–25 of 60");
  await screen.getByRole("button", { name: "Next" }).click();
  await expect.element(screen.getByRole("status")).toHaveTextContent("26–50 of 60");
  await screen.getByRole("button", { name: "Next" }).click();
  expect(screen.container.querySelectorAll("tbody tr")).toHaveLength(10);
  await expect.element(screen.getByRole("button", { name: "Next" })).toBeDisabled();
});
test("rejects duplicate identity instead of showing an ambiguous result", async () => {
  const row = { key: "same", label: "Option", cells: {} };
  const screen = await render(DataTable, {
    caption: "Options",
    columns: [],
    rows: [row, row],
    labels,
  });
  await expect.element(screen.getByRole("alert")).toHaveTextContent("Data unavailable");
  expect(screen.container.querySelector("table")).toBeNull();
});
