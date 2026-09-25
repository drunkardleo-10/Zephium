import "$styles/global.css";
import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import WorkCanvas from "../components/WorkCanvas.svelte";
import type { CanvasItem } from "../lib/canvas-model";

const note = (id: string, title: string): CanvasItem => ({
  id,
  type: "note",
  title,
  kind: "Notes",
  detail: "",
  status: "",
});

test("one capsule zooms: its number fits the view, 1 goes back to actual size", async () => {
  await page.viewport(1200, 800);
  const screen = await render(WorkCanvas, {
    items: [note("a", "Alpha"), note("b", "Beta")],
    links: [],
    authoritative: new Set(["a", "b"]),
    initialView: {
      positions: { a: { x: 0, y: 0 }, b: { x: 2600, y: 1800 } },
      viewport: { x: 0, y: 0, zoom: 1 },
    },
    oninspect: () => {},
  });
  screen.container.style.width = "1200px";
  screen.container.style.height = "800px";
  const capsule = screen.getByRole("group", { name: "Canvas controls" });
  const level = capsule.getByRole("button", { name: "Fit view" });
  await expect.element(level).toHaveTextContent("100%");
  // The pointer tools are gone: panning is the default, Shift draws a marquee.
  expect(screen.container.textContent).not.toContain("Select (V)");
  await expect.element(capsule.getByRole("button", { name: "Zoom in" })).toBeVisible();
  await expect.element(capsule.getByRole("button", { name: "Zoom out" })).toBeVisible();
  await level.click();
  await expect.poll(() => level.element().textContent).not.toBe("100%");
  const beta = () => screen.getByText("Beta", { exact: true }).element().getBoundingClientRect();
  await expect.poll(() => beta().right).toBeLessThanOrEqual(1200);
  expect(beta().bottom).toBeLessThanOrEqual(800);
  await userEvent.keyboard("1");
  await expect.poll(() => level.element().textContent).toBe("100%");
  await screen.unmount();
});
