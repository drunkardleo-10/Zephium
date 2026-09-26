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

test("the thread rests as a curve with a dot; a relation shows only while an end is hovered", async () => {
  await page.viewport(1200, 800);
  const items = [note("a", "Alpha"), note("b", "Beta"), note("c", "Gamma")];
  const links: CanvasLink[] = [
    { id: "relation:1", source: "a", target: "b", kind: "supports", label: "supports" },
    { id: "stage:c", source: "a", target: "c", kind: "thread" },
  ];
  const screen = await render(WorkCanvas, {
    items,
    links,
    authoritative: new Set(["a", "b", "c"]),
    initialView: {
      positions: { a: { x: 0, y: 0 }, b: { x: 400, y: 0 }, c: { x: 0, y: 360 } },
      viewport: { x: 40, y: 60, zoom: 0.8 },
    },
    oninspect: () => {},
  });
  screen.container.style.width = "1200px";
  screen.container.style.height = "800px";
  const edge = (id: string) =>
    screen.container.querySelector(`.svelte-flow__edge[data-id="${id}"]`);
  await expect.poll(() => edge("stage:c")).not.toBeNull();
  // A cubic, not a routed step: one C segment, a round cap.
  const line = edge("stage:c")!.querySelector<SVGPathElement>(".work-edge-line")!;
  expect(line.getAttribute("d")).toMatch(/^M [\d.-]+,[\d.-]+ C /u);
  expect(getComputedStyle(line).strokeLinecap).toBe("round");
  expect(edge("relation:1")).toBeNull();
  await screen.getByText("Alpha", { exact: true }).hover();
  await expect.poll(() => edge("relation:1")).not.toBeNull();
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

test("a card a live run adds rises in; the cards already there stay still", async () => {
  await page.viewport(1200, 800);
  const props = {
    items: [note("a", "Alpha"), note("b", "Beta")],
    links: [],
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
  await expect.poll(() => wrapper("b")).not.toBeNull();
  expect(wrapper("b")!.getAnimations()).toHaveLength(0);
  await screen.rerender({ ...props, items: [...props.items, note("c", "Gamma")] });
  await expect.poll(() => wrapper("c")?.getAnimations().length ?? 0).toBeGreaterThan(0);
  expect(wrapper("b")!.getAnimations()).toHaveLength(0);
  await screen.unmount();
});
