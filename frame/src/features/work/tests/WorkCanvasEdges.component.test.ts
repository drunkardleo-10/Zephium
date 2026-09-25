import "$styles/global.css";
import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import WorkCanvas from "../components/WorkCanvas.svelte";
import type { CanvasCluster, CanvasItem, CanvasLink } from "../lib/canvas-model";

const note = (id: string, title: string): CanvasItem => ({
  id,
  type: "note",
  title,
  kind: "Notes",
  detail: "",
  status: "",
});

test("group edges rest as curves with a dot; a relation shows only while an end is hovered", async () => {
  await page.viewport(1200, 800);
  const items = [note("a", "Alpha"), note("b", "Beta"), note("c", "Gamma")];
  const links: CanvasLink[] = [
    { id: "relation:1", source: "a", target: "b", kind: "supports", label: "supports" },
    { id: "path:a:group", source: "a", target: "group:notes", kind: "path" },
  ];
  const screen = await render(WorkCanvas, {
    items,
    links,
    clusters: [{ id: "group:notes", label: "2 notes", more: 0, members: ["b", "c"] }],
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
  await expect.poll(() => edge("path:a:group")).not.toBeNull();
  // A cubic, not a routed step: one C segment, a round cap, a 4 px dot at the group.
  const line = edge("path:a:group")!.querySelector<SVGPathElement>(".work-edge-line")!;
  expect(line.getAttribute("d")).toMatch(/^M [\d.-]+,[\d.-]+ C /u);
  expect(getComputedStyle(line).strokeLinecap).toBe("round");
  expect(edge("path:a:group")!.querySelector("circle")?.getAttribute("r")).toBe("2");
  expect(edge("relation:1")).toBeNull();
  // The group is a node around its members, never a card, with its caption inside.
  const cluster = screen.container.querySelector('.svelte-flow__node[data-id="group:notes"]');
  expect(cluster?.textContent).toContain("2 notes");
  await screen.getByText("Alpha", { exact: true }).hover();
  await expect.poll(() => edge("relation:1")).not.toBeNull();
  await expect.poll(() => cluster?.querySelector(".cluster.active")).toBeNull();
  await screen.getByText("Beta", { exact: true }).hover();
  await expect.poll(() => cluster?.querySelector(".cluster.active")).not.toBeNull();
  await screen.unmount();
});

test("the agent is a mark, not a card: an orb with its caption, above the cards", async () => {
  await page.viewport(1200, 800);
  const agent: CanvasItem = {
    id: "agent:request",
    type: "agent",
    title: "Agent",
    kind: "Agent",
    detail: "",
    status: "Reading airbnb.com",
    agent: {
      seed: 7,
      activity: "reading",
      objective: "objective",
      doing: "reading",
      caption: "Reading airbnb.com",
      stand: { x: 300, y: 20 },
    },
  };
  const screen = await render(WorkCanvas, {
    items: [note("a", "Alpha"), agent],
    links: [],
    authoritative: new Set(["a"]),
    initialView: {
      positions: { a: { x: 0, y: 0 }, "agent:request": { x: 300, y: 20 } },
      viewport: { x: 0, y: 0, zoom: 1 },
    },
    oninspect: () => {},
  });
  screen.container.style.width = "1200px";
  screen.container.style.height = "800px";
  const node = () =>
    screen.container.querySelector<HTMLElement>('.svelte-flow__node[data-id="agent:request"]');
  await expect
    .poll(() => node()?.querySelector(".caption")?.textContent)
    .toBe("Reading airbnb.com");
  expect(node()!.classList.contains("svelte-flow__node-agent")).toBe(true);
  expect(node()!.querySelector("article, .card")).toBeNull();
  expect(node()!.querySelector(".svelte-flow__handle")).toBeNull();
  expect(node()!.getBoundingClientRect().width).toBe(24);
  expect(Number(node()!.style.zIndex)).toBeGreaterThan(0);
  await screen.unmount();
});

test("a group a live run adds rises in and its edge draws; the groups already there stay still", async () => {
  await page.viewport(1200, 800);
  const items = [note("a", "Alpha"), note("b", "Beta"), note("c", "Gamma")];
  const first: CanvasCluster[] = [
    { id: "group:one", label: "1 note", more: 0, members: ["b"], live: true },
  ];
  const links: CanvasLink[] = [{ id: "path:one", source: "a", target: "group:one", kind: "path" }];
  const props = {
    items,
    links,
    clusters: first,
    authoritative: new Set(["a", "b", "c"]),
    initialView: {
      positions: { a: { x: 0, y: 0 }, b: { x: 400, y: 0 }, c: { x: 800, y: 0 } },
      viewport: { x: 0, y: 0, zoom: 0.8 },
    },
    oninspect: () => {},
  };
  const screen = await render(WorkCanvas, props);
  screen.container.style.width = "1200px";
  screen.container.style.height = "800px";
  const wrapper = (id: string) =>
    screen.container.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${id}"]`);
  const edge = (id: string) =>
    screen.container.querySelector<SVGPathElement>(
      `.svelte-flow__edge[data-id="${id}"] .work-edge-line`,
    );
  await expect.poll(() => edge("path:one")).not.toBeNull();
  expect(wrapper("group:one")!.getAnimations()).toHaveLength(0);
  expect(edge("path:one")!.getAnimations()).toHaveLength(0);
  await screen.rerender({
    ...props,
    clusters: [...first, { id: "group:two", label: "1 note", more: 0, members: ["c"], live: true }],
    links: [...links, { id: "path:two", source: "group:one", target: "group:two", kind: "path" }],
  });
  await expect.poll(() => wrapper("group:two")?.getAnimations().length ?? 0).toBeGreaterThan(0);
  await expect.poll(() => edge("path:two")?.getAnimations().length ?? 0).toBeGreaterThan(0);
  expect(edge("path:two")!.getAttribute("pathLength")).toBe("1");
  expect(wrapper("group:one")!.getAnimations()).toHaveLength(0);
  await screen.unmount();
});
