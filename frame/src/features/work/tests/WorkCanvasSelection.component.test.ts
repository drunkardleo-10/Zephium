import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import WorkCanvas from "../components/WorkCanvas.svelte";
import type { CanvasItem, CanvasView } from "../lib/canvas-model";
import { marquee } from "./marquee";

const note = (id: string, title: string, area?: string): CanvasItem => ({
  id,
  type: "note",
  title,
  kind: "Notes",
  detail: "",
  status: "",
  ...(area ? { area } : {}),
});

async function canvas(
  items: CanvasItem[],
  positions: CanvasView["positions"],
  extra: Record<string, unknown> = {},
) {
  await page.viewport(1200, 800);
  const screen = await render(WorkCanvas, {
    items,
    links: [],
    authoritative: new Set(items.map((item) => item.id)),
    initialView: { positions, viewport: { x: 0, y: 0, zoom: 1 } },
    oninspect: () => {},
    ...extra,
  });
  screen.container.style.width = "1200px";
  screen.container.style.height = "800px";
  return screen;
}

const origin = (container: HTMLElement) =>
  container.querySelector(".svelte-flow")!.getBoundingClientRect();
const selected = (container: HTMLElement) =>
  [...container.querySelectorAll(".svelte-flow__node.selected")].map((node) =>
    node.getAttribute("data-id"),
  );

test("a Shift marquee selects the cards it touches and one bar stands over them", async () => {
  const screen = await canvas([note("a", "Alpha"), note("b", "Beta"), note("c", "Gamma")], {
    a: { x: 40, y: 200 },
    b: { x: 420, y: 200 },
    c: { x: 820, y: 200 },
  });
  await expect.poll(() => screen.container.querySelectorAll(".work-drag-handle").length).toBe(3);
  const box = origin(screen.container);
  await userEvent.keyboard("{Shift>}");
  // Touching Beta's left edge is enough: the marquee selects partially covered cards.
  await marquee(screen.container, [box.left + 20, box.top + 120], [box.left + 460, box.top + 460]);
  await userEvent.keyboard("{/Shift}");
  await expect.poll(() => selected(screen.container).sort()).toEqual(["a", "b"]);
  await expect.element(screen.getByRole("toolbar", { name: "2 selected" })).toBeVisible();
  await userEvent.keyboard("{Escape}");
  await expect.poll(() => selected(screen.container)).toEqual([]);
  // Shift or Cmd with a click adds a card to the selection.
  await screen.getByText("Alpha", { exact: true }).click();
  await userEvent.keyboard("{Shift>}");
  await screen.getByText("Gamma", { exact: true }).click();
  await userEvent.keyboard("{/Shift}");
  await expect.poll(() => selected(screen.container).sort()).toEqual(["a", "c"]);
  await userEvent.keyboard("{Escape}");
  await expect.poll(() => selected(screen.container)).toEqual([]);
  await expect.element(screen.getByRole("toolbar", { name: "2 selected" })).not.toBeInTheDocument();
  await screen.unmount();
});

test("Grid arranges three cards into two columns and publishes the placements", async () => {
  const onviewchange = vi.fn();
  const screen = await canvas(
    [note("a", "Alpha"), note("b", "Beta"), note("c", "Gamma")],
    { a: { x: 40, y: 100 }, b: { x: 420, y: 120 }, c: { x: 800, y: 140 } },
    { onviewchange },
  );
  await expect.poll(() => screen.container.querySelectorAll(".work-drag-handle").length).toBe(3);
  // The select tool draws the marquee without a key.
  await screen.getByRole("button", { name: "Select (V)" }).click();
  const box = origin(screen.container);
  await marquee(screen.container, [box.left + 20, box.top + 60], [box.left + 1140, box.top + 520]);
  await expect.poll(() => selected(screen.container).length).toBe(3);
  await screen.getByRole("button", { name: "Arrange" }).click();
  await page.getByRole("menuitem", { name: "Grid" }).click();
  await expect
    .poll(() => onviewchange.mock.lastCall?.[0].positions)
    .toEqual({
      a: { x: 40, y: 100 },
      b: { x: 360, y: 100 },
      c: { x: 40, y: 320 },
    });
  await screen.getByRole("button", { name: "Hand (H)" }).click();
  await screen.unmount();
});

test("double-clicking an area's title renames it in place on Enter", async () => {
  const onareaedit = vi.fn();
  const screen = await canvas(
    [note("a", "Alpha", "g")],
    { a: { x: 200, y: 200 } },
    {
      areas: [{ id: "g", title: "Group" }],
      onareaedit,
    },
  );
  const title = screen.container.querySelector<HTMLElement>(".area-title")!;
  await expect.poll(() => title.textContent).toContain("Group");
  title.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  const input = screen.getByRole("textbox", { name: "Area title" });
  await expect.element(input).toHaveFocus();
  await input.fill("Shortlist");
  await userEvent.keyboard("{Enter}");
  expect(onareaedit.mock.calls).toEqual([["g", { kind: "rename", title: "Shortlist" }]]);
  await expect.element(input).not.toBeInTheDocument();
  await screen.unmount();
});
