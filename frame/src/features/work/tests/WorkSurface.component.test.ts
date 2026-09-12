import { expect, test, vi } from "vitest";
import { userEvent } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import { loadWorkSurface, loadWorkCanvas, type CanvasView, type WorkSurfaceView } from "../index";
const view: WorkSurfaceView = {
  key: "work-1",
  title: "Research",
  objective: "Compare options",
  phase: "Needs review",
  questions: [],
  items: [
    {
      id: "one",
      title: "Inspect evidence",
      kind: "Resource",
      detail: "Captured sources",
      status: "Available",
    },
  ],
  links: [],
  artifacts: [],
  actions: [
    {
      key: "approve",
      label: "Review plan",
      scope: "Exact revision 7",
      consequence: "A bounded request, not proof of success.",
    },
  ],
};
test("canvas restores a bounded viewport and provides keyboard inspection", async () => {
  const { default: Canvas } = await loadWorkCanvas();
  const inspect = vi.fn();
  const initialView: CanvasView = {
    positions: { one: { x: 20, y: 30 } },
    viewport: { x: 40, y: 50, zoom: 0.75 },
  };
  const screen = await render(Canvas, {
    items: view.items,
    links: [],
    initialView,
    oninspect: inspect,
  });
  await expect
    .element(screen.getByRole("button", { name: "Inspect Inspect evidence", exact: true }))
    .toBeVisible();
  screen.getByRole("button", { name: "Inspect Inspect evidence", exact: true }).element().focus();
  await userEvent.keyboard("{Enter}");
  expect(inspect).toHaveBeenCalledExactlyOnceWith("one");
  expect(
    screen.container.querySelector<HTMLElement>(".svelte-flow__viewport")?.style.transform,
  ).toContain("0.75");
});
test("requires explicit confirmation and blocks unknown-outcome resubmission", async () => {
  const { default: Work } = await loadWorkSurface();
  const onintent = vi.fn();
  const screen = await render(Work, {
    view,
    request: { state: "ready", message: "Ready" },
    onintent,
  });
  await screen.getByRole("button", { name: "Review plan", exact: true }).click();
  expect(onintent).not.toHaveBeenCalled();
  await screen.getByRole("button", { name: "Confirm request" }).click();
  expect(onintent).toHaveBeenCalledExactlyOnceWith({ kind: "action", key: "approve" });
  await screen.rerender({ request: { state: "unknown", message: "Reconciliation required" } });
  await expect
    .element(screen.getByRole("button", { name: "Review plan", exact: true }))
    .toBeDisabled();
  await screen.rerender({ active: false });
  expect(screen.container.querySelector(".svelte-flow")).toBeNull();
  expect(screen.container.querySelector("textarea")).toBeNull();
});

test("invalidates open confirmations when the presentation changes", async () => {
  const { default: Work } = await loadWorkSurface();
  const onintent = vi.fn();
  const screen = await render(Work, {
    view,
    request: { state: "ready", message: "Ready" },
    onintent,
  });
  await screen.getByRole("button", { name: "Review plan", exact: true }).click();
  await expect.element(screen.getByRole("button", { name: "Confirm request" })).toBeVisible();
  await screen.rerender({ view: { ...view, phase: "Changed" } });
  await expect
    .element(screen.getByRole("button", { name: "Confirm request" }))
    .not.toBeInTheDocument();
  expect(onintent).not.toHaveBeenCalled();
});

test("contains asynchronous dispatch failure and requires reconciliation", async () => {
  const { default: Work } = await loadWorkSurface();
  const onintent = vi.fn().mockRejectedValue(new Error("transport disconnected"));
  const screen = await render(Work, {
    view,
    request: { state: "ready", message: "Ready" },
    onintent,
    onrefresh: vi.fn(),
  });
  await screen.getByRole("button", { name: "Review plan", exact: true }).click();
  await screen.getByRole("button", { name: "Confirm request" }).click();
  await expect
    .element(
      screen.getByText("Delivery could not be confirmed. Refresh current state before retrying."),
    )
    .toBeVisible();
  await expect
    .element(screen.getByRole("button", { name: "Review plan", exact: true }))
    .toBeDisabled();
});

test("empty Work accepts an objective intent and keeps drafts across hide/show", async () => {
  const { default: Work } = await loadWorkSurface();
  const onintent = vi.fn();
  const empty: WorkSurfaceView = { ...view, objective: "", items: [], actions: [] };
  const screen = await render(Work, {
    view: empty,
    request: { state: "ready", message: "Ready" },
    onintent,
  });
  await screen
    .getByRole("textbox", { name: "Objective", exact: true })
    .fill("Investigate a new topic");
  await screen.rerender({ active: false });
  await screen.rerender({ active: true });
  await expect
    .element(screen.getByRole("textbox", { name: "Objective", exact: true }))
    .toHaveValue("Investigate a new topic");
  await screen.getByRole("button", { name: "Submit objective" }).click();
  expect(onintent).toHaveBeenCalledExactlyOnceWith({
    kind: "objective",
    text: "Investigate a new topic",
  });
});

test("bounds offscreen rendering for a 500-item restored workspace", async () => {
  const { default: Canvas } = await loadWorkCanvas();
  const items = Array.from({ length: 500 }, (_, i) => ({
    id: `item-${i}`,
    title: `Item ${i}`,
    kind: "Resource",
    detail: "Description",
    status: "Available",
  }));
  const started = performance.now();
  const screen = await render(Canvas, {
    items,
    links: [],
    initialView: { positions: {}, viewport: { x: 0, y: 0, zoom: 1 } },
    oninspect: vi.fn(),
  });
  await expect
    .poll(() => screen.container.querySelectorAll(".svelte-flow__node").length)
    .toBeLessThan(500);
  const count = screen.container.querySelectorAll(".svelte-flow__node").length;
  console.warn(
    `Work canvas 500-item mount: ${Math.round(performance.now() - started)}ms, ${count} rendered nodes`,
  );
  expect(count).toBeGreaterThan(0);
  expect(count).toBeLessThan(500);
});

test("clarification options and typed answers react for opaque question keys", async () => {
  const { default: Work } = await loadWorkSurface();
  const onintent = vi.fn();
  const screen = await render(Work, {
    view: {
      ...view,
      questions: [{ key: "__proto__", prompt: "Choose a budget", options: ["Under 150"] }],
    },
    request: { state: "ready", message: "Ready" },
    onintent,
  });
  await screen.getByRole("button", { name: "Under 150", exact: true }).click();
  await expect
    .element(screen.getByRole("textbox", { name: "Choose a budget" }))
    .toHaveValue("Under 150");
  await screen.getByRole("textbox", { name: "Choose a budget" }).fill("Prefer location");
  await screen.getByRole("button", { name: "Submit answer" }).click();
  expect(onintent).toHaveBeenCalledExactlyOnceWith({
    kind: "answer",
    question: "__proto__",
    text: "Prefer location",
  });
});

test("hidden Work releases pending observation without treating the effect as cancelled", async () => {
  const { default: Work } = await loadWorkSurface();
  const screen = await render(Work, {
    view,
    request: { state: "ready", message: "Ready" },
    onintent: () => new Promise<void>(() => {}),
    onrefresh: vi.fn(),
  });
  await screen.getByRole("button", { name: "Review plan", exact: true }).click();
  await screen.getByRole("button", { name: "Confirm request" }).click();
  await screen.rerender({ active: false });
  await screen.rerender({ active: true });
  await expect
    .element(
      screen.getByText("Delivery could not be confirmed. Refresh current state before retrying."),
    )
    .toBeVisible();
  await expect.element(screen.getByRole("button", { name: "Refresh current state" })).toBeEnabled();
});
