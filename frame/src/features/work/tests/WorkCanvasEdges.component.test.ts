import "$styles/global.css";
import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import WorkCanvas from "../components/WorkCanvas.svelte";
import type { CanvasItem, CanvasLink } from "../lib/canvas-model";

const note = (id: string, title: string): CanvasItem => ({
  id,
  type: "note",
  title,
  kind: "Notes",
  detail: "",
  status: "",
});

test("a relation shows only while one of its ends is hovered; the path stays at rest", async () => {
  await page.viewport(1200, 800);
  const items = [note("a", "Alpha"), note("b", "Beta"), note("c", "Gamma")];
  const links: CanvasLink[] = [
    { id: "relation:1", source: "a", target: "b", kind: "supports", label: "supports" },
    { id: "path:a:b", source: "a", target: "c", kind: "path" },
  ];
  const screen = await render(WorkCanvas, {
    items,
    links,
    clusters: [{ id: "cluster:notes", label: "2 notes", more: 0, members: ["b", "c"] }],
    authoritative: new Set(["a", "b", "c"]),
    initialView: {
      positions: { a: { x: 0, y: 0 }, b: { x: 400, y: 0 }, c: { x: 400, y: 260 } },
      viewport: { x: 40, y: 60, zoom: 0.8 },
    },
    oninspect: () => {},
  });
  screen.container.style.width = "1200px";
  screen.container.style.height = "800px";
  const edge = (id: string) =>
    screen.container.querySelector(`.svelte-flow__edge[data-id="${id}"]`);
  await expect.poll(() => edge("relation:1")?.classList.contains("latent")).toBe(true);
  expect(edge("relation:1")?.classList.contains("active")).toBe(false);
  expect(edge("path:a:b")?.classList.contains("latent")).toBe(false);
  // The cluster is a derived node around its members, never a card.
  const cluster = screen.container.querySelector('.svelte-flow__node[data-id="cluster:notes"]');
  expect(cluster?.textContent).toContain("2 notes");
  await screen.getByText("Alpha", { exact: true }).hover();
  await expect.poll(() => edge("relation:1")?.classList.contains("active")).toBe(true);
  await expect.poll(() => cluster?.querySelector(".cluster.active")).toBeNull();
  await screen.getByText("Beta", { exact: true }).hover();
  await expect.poll(() => cluster?.querySelector(".cluster.active")).not.toBeNull();
  await screen.unmount();
});
