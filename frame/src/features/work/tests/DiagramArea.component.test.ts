import "$styles/global.css";
import { afterEach, expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import { favicons } from "$domain/favicons";
import type { WorkArtifactDataV1 } from "$shared/ipc/bindings";
import type { ArtifactContent } from "$shared/ui/data/Artifact";
import WorkCanvas from "../components/WorkCanvas.svelte";
import DiagramLift from "../components/DiagramLift.svelte";
import { environmentStages, environmentRequests } from "../lib/project-environment-thread";
import { environmentClusters, environmentDiagrams, resultHeads } from "../lib/project-environment";
import { artifactView } from "../lib/project-work";
import { layoutDiagram } from "../lib/diagram";
import { projection, snapshot } from "./environment-fixtures";

afterEach(() => favicons.dispose());

/** Five parts in two layers, six flows: two between the same parts, one back to the edge. */
const FIVE: WorkArtifactDataV1 = {
  kind: "diagram",
  nodes: [
    { id: "web", name: "Browser", kind: "client", layer: "edge" },
    { id: "cdn", name: "CDN", kind: "edge", layer: "edge" },
    { id: "api", name: "API", kind: "service", layer: "app" },
    { id: "jobs", name: "Jobs", kind: "worker", layer: "app" },
    { id: "db", name: "Store", kind: "store", layer: "app" },
  ],
  edges: [
    { from: "web", to: "api", label: "HTTPS" },
    { from: "api", to: "jobs", label: "enqueues" },
    { from: "jobs", to: "web", label: "push" },
    { from: "api", to: "db", label: "SQL" },
    { from: "db", to: "api", label: "rows" },
    { from: "web", to: "cdn" },
  ],
  layers: [
    { id: "edge", name: "Edge" },
    { id: "app", name: "Application" },
  ],
};

/** The lane as the workspace projects it, once the layout engine has answered. */
async function scene(data: WorkArtifactDataV1) {
  const state = structuredClone(projection);
  const execution = state.executions[0]!;
  execution.artifacts[0]!.data = data;
  execution.artifacts[0]!.title = "Five parts";
  execution.user_artifacts = [];
  const view = artifactView(execution.artifacts[0]!, execution);
  await layoutDiagram(view.content as Extract<ArtifactContent, { kind: "diagram" }>);
  const objectives = new Map([["objective", state]]);
  const stages = environmentStages(snapshot, objectives);
  const diagrams = environmentDiagrams(snapshot, objectives, stages);
  const lanes = environmentClusters(stages, resultHeads(snapshot, objectives, stages));
  return {
    items: diagrams.items,
    links: [...lanes.links, ...diagrams.links],
    clusters: [...diagrams.clusters, ...lanes.clusters],
    positions: { ...environmentRequests(stages).positions, ...diagrams.positions },
  };
}

const LABELS = ["HTTPS", "enqueues", "push", "SQL", "rows"];
/** The whole lane in view, so the pointer can reach every part. */
const ZOOM = 0.6;

test("a diagram draws lanes as bands, flows as elbows with arrowheads, and names on their lines", async () => {
  await page.viewport(1400, 900);
  const { items, links, clusters, positions } = await scene(FIVE);
  const screen = await render(WorkCanvas, {
    items,
    links,
    clusters,
    authoritative: new Set(["result-card"]),
    initialView: { positions, viewport: { x: 0, y: 40, zoom: ZOOM } },
    oninspect: () => {},
  });
  screen.container.style.width = "1400px";
  screen.container.style.height = "900px";
  const root = screen.container;
  const rect = (element: Element) => element.getBoundingClientRect();
  const part = (id: string) =>
    root.querySelector<HTMLElement>(`.svelte-flow__node[data-id="diagram:result-card:${id}"]`)!;
  await expect.poll(() => root.querySelectorAll(".work-flow-line").length).toBe(6);
  // Each layer is a soft band, captioned, holding its parts.
  const bands = [...root.querySelectorAll<HTMLElement>(".svelte-flow__node-band")];
  expect(bands.map((band) => band.textContent?.trim())).toEqual(["Edge", "Application"]);
  const lane = (band: HTMLElement, ids: string[]) => {
    for (const id of ids) {
      const inner = rect(part(id));
      expect(inner.left).toBeGreaterThanOrEqual(rect(band).left);
      expect(inner.right).toBeLessThanOrEqual(rect(band).right);
    }
  };
  lane(bands[0]!, ["web", "cdn"]);
  lane(bands[1]!, ["api", "jobs", "db"]);
  // Elbow lines with rounded corners, each ending in an arrowhead.
  const lines = [...root.querySelectorAll<SVGPathElement>(".work-flow-line")];
  expect(lines.some((line) => line.getAttribute("d")!.includes(" Q "))).toBe(true);
  expect(root.querySelectorAll(".work-flow-head")).toHaveLength(6);
  // Every name at rest sits on its own line, clear of every part.
  await expect
    .poll(() => [...document.querySelectorAll(".work-edge-label")].map((l) => l.textContent))
    .toEqual(expect.arrayContaining(LABELS));
  const flow = root.querySelector<HTMLElement>(".svelte-flow")!;
  const toFlow = (x: number, y: number) =>
    new DOMPoint((x - rect(flow).left) / ZOOM, (y - rect(flow).top - 40) / ZOOM);
  const cards = ["web", "cdn", "api", "jobs", "db"].map((id) => rect(part(id)));
  for (const [index, name] of LABELS.entries()) {
    const plate = [...document.querySelectorAll<HTMLElement>(".work-edge-label")].find(
      (label) => label.textContent === name,
    )!;
    expect(plate.title).toBe(name);
    const box = rect(plate);
    const line = root.querySelector<SVGPathElement>(
      `.svelte-flow__edge[data-id="diagram-edge:result-card:${index}"] .work-flow-line`,
    )!;
    let crossed = false;
    for (let x = box.left + 1; x < box.right && !crossed; x += 1)
      for (let y = box.top + 1; y < box.bottom && !crossed; y += 1)
        crossed = line.isPointInStroke(toFlow(x, y));
    expect(crossed, `"${name}" on its line`).toBe(true);
    for (const card of cards)
      expect(
        box.left < card.right &&
          card.left < box.right &&
          box.top < card.bottom &&
          card.top < box.bottom,
        `"${name}" over a part`,
      ).toBe(false);
  }
  // The area holds the whole picture.
  const area = rect(
    root.querySelector('.svelte-flow__node[data-id^="group:"][data-id$=":diagram:result-card"]')!,
  );
  for (const inner of [...bands.map(rect), ...cards]) {
    expect(inner.left).toBeGreaterThanOrEqual(area.left - 0.5);
    expect(inner.right).toBeLessThanOrEqual(area.right + 0.5);
    expect(inner.top).toBeGreaterThanOrEqual(area.top - 0.5);
    expect(inner.bottom).toBeLessThanOrEqual(area.bottom + 0.5);
  }
  await screen.unmount();
});

test("hovering a part lights its flows and names, quiets the rest, and dims the parts it does not reach", async () => {
  await page.viewport(1400, 900);
  const { items, links, clusters, positions } = await scene(FIVE);
  const screen = await render(WorkCanvas, {
    items,
    links,
    clusters,
    authoritative: new Set(["result-card"]),
    initialView: { positions, viewport: { x: 0, y: 40, zoom: ZOOM } },
    oninspect: () => {},
  });
  screen.container.style.width = "1400px";
  screen.container.style.height = "900px";
  const root = screen.container;
  const state = (index: number) =>
    root.querySelector(`.svelte-flow__edge[data-id="diagram-edge:result-card:${index}"] .work-flow`)
      ?.classList;
  await expect.poll(() => state(5)?.contains("quiet")).toBe(true);
  const names = () =>
    [...document.querySelectorAll(".work-edge-label")].map((label) => label.textContent).sort();
  expect(names()).toEqual([...LABELS].sort());
  await screen.getByText("Jobs", { exact: true }).hover();
  await expect.poll(() => state(1)?.contains("lit"), { timeout: 3000 }).toBe(true);
  expect(state(2)?.contains("lit")).toBe(true);
  for (const index of [0, 3, 4, 5]) expect(state(index)?.contains("quiet")).toBe(true);
  await expect.poll(names).toEqual(["enqueues", "push"]);
  const dimmed = () =>
    [...root.querySelectorAll(".part.dimmed")].map((element) =>
      element.closest(".svelte-flow__node")!.getAttribute("data-id"),
    );
  expect(dimmed().sort()).toEqual(["diagram:result-card:cdn", "diagram:result-card:db"]);
  // Leaving restores the picture at rest.
  await page
    .elementLocator(root.querySelector(".svelte-flow__pane")!)
    .hover({ position: { x: 5, y: 5 } });
  await expect.poll(names).toEqual([...LABELS].sort());
  expect(dimmed()).toEqual([]);
  // The arrow keys walk the parts along their flows.
  const node = (id: string) =>
    root.querySelector<HTMLElement>(`.svelte-flow__node[data-id="diagram:result-card:${id}"]`)!;
  node("web").focus();
  await expect.poll(() => state(0)?.contains("lit")).toBe(true);
  const press = (id: string, key: string) =>
    node(id).dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  const focusedId = () => document.activeElement?.getAttribute("data-id");
  press("web", "ArrowRight");
  await expect.poll(focusedId).not.toBe("diagram:result-card:web");
  const reached = focusedId()!;
  expect(["diagram:result-card:api", "diagram:result-card:cdn"]).toContain(reached);
  press(reached.slice("diagram:result-card:".length), "ArrowLeft");
  await expect.poll(focusedId).toBe("diagram:result-card:web");
  await screen.unmount();
});

test("the lift shows the picture larger, lists parts and flows, and a picked row lights its part or flow", async () => {
  const content = {
    kind: "diagram",
    nodes: FIVE.kind === "diagram" ? FIVE.nodes : [],
    edges: FIVE.kind === "diagram" ? FIVE.edges : [],
    layers: FIVE.kind === "diagram" ? (FIVE.layers ?? []) : [],
  } as Extract<ArtifactContent, { kind: "diagram" }>;
  await layoutDiagram(content);
  const screen = await render(DiagramLift, { content });
  const root = screen.container;
  const scaled = root.querySelector<HTMLElement>(".scaled")!;
  expect(scaled.style.transform).toBe("scale(1.25)");
  expect(root.querySelectorAll(".band")).toHaveLength(2);
  expect(root.querySelectorAll(".flows .flow")).toHaveLength(6);
  const rows = [...root.querySelectorAll<HTMLButtonElement>(".lists button")];
  expect(rows.map((row) => row.textContent?.replace(/\s+/gu, " ").trim())).toEqual([
    "Browser Client",
    "CDN Edge",
    "API Service",
    "Jobs Worker",
    "Store Database",
    "Browser → API: HTTPS",
    "API → Jobs: enqueues",
    "Jobs → Browser: push",
    "API → Store: SQL",
    "Store → API: rows",
    "Browser → CDN",
  ]);
  const flows = () => [...root.querySelectorAll(".flows .flow")].map((flow) => flow.classList);
  // A flow's row lights that flow alone and dims every part it does not join.
  await page.elementLocator(rows[8]!).click();
  await expect.poll(() => flows()[3]!.contains("lit")).toBe(true);
  expect(flows().filter((flow) => flow.contains("lit"))).toHaveLength(1);
  expect(root.querySelectorAll(".part.dimmed")).toHaveLength(3);
  expect([...root.querySelectorAll(".plate")].map((plate) => plate.textContent)).toEqual(["SQL"]);
  // A part's row lights all its flows.
  await page.elementLocator(rows[3]!).click();
  await expect
    .poll(() => flows().flatMap((flow, index) => (flow.contains("lit") ? [index] : [])))
    .toEqual([1, 2]);
  expect(rows[3]!.getAttribute("aria-pressed")).toBe("true");
  // Picking it again returns the picture to rest.
  await page.elementLocator(rows[3]!).click();
  await expect.poll(() => root.querySelectorAll(".part.dimmed").length).toBe(0);
  await screen.unmount();
});
