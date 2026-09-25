import "$styles/global.css";
import { afterEach, expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import { favicons } from "$domain/favicons";
import { emitNativeEvent } from "$shared/testing/native-events";
import type { WorkArtifactDataV1 } from "$shared/ipc/bindings";
import WorkCanvas from "../components/WorkCanvas.svelte";
import DiagramNodeCard from "../components/cards/DiagramNodeCard.svelte";
import ResultCard from "../components/cards/ResultCard.svelte";
import { environmentStages, environmentRequests } from "../lib/project-environment-thread";
import { environmentClusters, environmentDiagrams } from "../lib/project-environment";
import { artifactView } from "../lib/project-work";
import type { CanvasItem } from "../lib/canvas-model";
import { projection, snapshot } from "./environment-fixtures";

afterEach(() => favicons.dispose());

const DIAGRAM: WorkArtifactDataV1 = {
  kind: "diagram",
  nodes: [
    { id: "db", name: "Postgres", kind: "store", vendor: "postgresql.org", layer: "data" },
    { id: "web", name: "Browser", kind: "client", layer: "edge" },
    { id: "api", name: "API", kind: "service", note: "Rust, axum", layer: "app" },
    { id: "jobs", name: "Jobs", kind: "worker", layer: "app" },
  ],
  edges: [
    { from: "web", to: "api", label: "HTTPS" },
    { from: "api", to: "db", label: "SQL" },
    { from: "api", to: "jobs" },
  ],
  layers: [
    { id: "edge", name: "Edge" },
    { id: "app", name: "Application" },
    { id: "data", name: "Data" },
  ],
};

function scene(data: WorkArtifactDataV1) {
  const state = structuredClone(projection);
  const execution = state.executions[0]!;
  execution.artifacts[0]!.data = data;
  execution.artifacts[0]!.title = "Checkout system";
  execution.user_artifacts = [];
  const objectives = new Map([["objective", state]]);
  const stages = environmentStages(snapshot, objectives);
  const diagrams = environmentDiagrams(snapshot, objectives, stages);
  const lanes = environmentClusters(stages);
  const view = artifactView(execution.artifacts[0]!, execution);
  const cover: CanvasItem = {
    id: "result-card",
    type: "result",
    title: view.title,
    kind: "Result",
    detail: "",
    status: "",
    artifact: view,
  };
  return {
    items: [cover, ...diagrams.items],
    links: [...lanes.links, ...diagrams.links],
    clusters: [...diagrams.clusters, ...lanes.clusters],
    positions: { ...environmentRequests(stages).positions, ...diagrams.positions },
  };
}

test("a diagram result is its cover beside an area of parts, layered left to right and joined", async () => {
  await page.viewport(1400, 900);
  const { items, links, clusters, positions } = scene(DIAGRAM);
  const screen = await render(WorkCanvas, {
    items,
    links,
    clusters,
    authoritative: new Set(["result-card"]),
    initialView: { positions, viewport: { x: 0, y: 40, zoom: 0.8 } },
    oninspect: () => {},
  });
  screen.container.style.width = "1400px";
  screen.container.style.height = "900px";
  const area = () =>
    screen.container.querySelector(
      '.svelte-flow__node[data-id="group:objective-card:diagram:result-card"]',
    );
  await expect.poll(() => area()?.querySelector(".take")?.textContent).toBe("Checkout system");
  const parts = [...screen.container.querySelectorAll<HTMLElement>(".part")];
  expect(parts.map((part) => part.querySelector(".name")?.textContent)).toEqual(
    expect.arrayContaining(["Postgres", "Browser", "API", "Jobs"]),
  );
  expect(parts).toHaveLength(4);
  // The layers read left to right, each captioned above its column.
  const layers = [...area()!.querySelectorAll<HTMLElement>(".layer")];
  expect(layers.map((layer) => layer.textContent)).toEqual(["Edge", "Application", "Data"]);
  const x = (id: string) =>
    screen.container
      .querySelector<HTMLElement>(`.svelte-flow__node[data-id="diagram:result-card:${id}"]`)!
      .getBoundingClientRect().left;
  expect(x("web")).toBeLessThan(x("api"));
  expect(x("api")).toBe(x("jobs"));
  expect(x("api")).toBeLessThan(x("db"));
  // Every flow draws at rest, its label on a plate.
  await expect
    .poll(() => screen.container.querySelectorAll('.svelte-flow__edge[data-id^="diagram-edge:"]'))
    .toHaveLength(3);
  expect(
    [...document.querySelectorAll(".work-edge-label")].map((label) => label.textContent),
  ).toEqual(expect.arrayContaining(["HTTPS", "SQL"]));
  // The caption takes the diagram whole.
  await screen.getByTitle("Select the whole diagram").click();
  await expect
    .poll(() => screen.container.querySelectorAll(".svelte-flow__node.selected").length)
    .toBe(4);
  await screen.unmount();
});

const part = (vendor?: string): CanvasItem => ({
  id: "diagram:result-card:db",
  type: "diagram",
  kind: "Database",
  title: "Postgres",
  detail: "",
  status: "",
  diagram: { kind: "store", ...(vendor ? { vendor } : {}) },
});

test("a part shows its vendor's icon once it is held, and its kind's glyph until then", async () => {
  await favicons.init();
  const bare = await render(DiagramNodeCard, { item: part("postgresql.org"), selected: false });
  expect(bare.container.querySelector(".mark canvas")).toBeNull();
  expect(bare.container.querySelector(".mark svg")).not.toBeNull();
  expect(bare.container.querySelector(".caption")?.textContent).toBe("Database");
  await bare.unmount();
  let binary = "";
  for (let index = 0; index < 32 * 32; index += 1) binary += String.fromCharCode(51, 103, 145, 255);
  emitNativeEvent("favicons", {
    surface: "chrome",
    profile_id: "p",
    entries: [{ origin: "https://postgresql.org", revision: "a", rgba: btoa(binary) }],
  });
  const held = await render(DiagramNodeCard, { item: part("postgresql.org"), selected: false });
  await expect.poll(() => held.container.querySelector(".mark canvas")).not.toBeNull();
  await held.unmount();
});

const result = (data: WorkArtifactDataV1): CanvasItem => {
  const state = structuredClone(projection);
  const execution = state.executions[0]!;
  execution.artifacts[0]!.data = data;
  execution.user_artifacts = [];
  const view = artifactView(execution.artifacts[0]!, execution);
  return {
    id: "result",
    type: "result",
    title: "Quotes",
    kind: "",
    detail: "",
    status: "",
    artifact: view,
  };
};

test("a table card is a real table: six rows, figures to the right, then how many more", async () => {
  await page.viewport(1200, 800);
  const rows = Array.from({ length: 9 }, (_, index) => [
    `Builder ${index + 1}`,
    `$${(index + 3) * 1000}`,
    "Replaces the whole roof and the gutters, with a ten year warranty",
  ]);
  const screen = await render(ResultCard, {
    id: "result",
    item: result({ kind: "table", columns: ["Builder", "Quote", "Why"], rows }),
    selected: false,
    onaction: () => {},
  });
  const body = screen.container.querySelectorAll(".mini tbody tr");
  expect(body).toHaveLength(6);
  expect(screen.container.querySelectorAll(".mini thead th")).toHaveLength(3);
  const quote = screen.container.querySelector<HTMLElement>(".mini tbody td:nth-child(2)")!;
  expect(getComputedStyle(quote).textAlign).toBe("end");
  await expect.element(screen.getByText("+3 rows")).toBeVisible();
  expect(screen.container.textContent).not.toContain("Builder 7");
  await screen.unmount();
});

const chart = (general_knowledge: boolean): WorkArtifactDataV1 => ({
  kind: "chart",
  x_label: "Builder",
  y_label: "Quote",
  series: [
    {
      name: "Quote",
      points: [
        { label: "Acme", value: "3200", evidence: general_knowledge ? [] : [0] },
        { label: "Brick", value: "4100" },
      ],
    },
  ],
  general_knowledge,
});

test("a chart card is compact, and says it is from what the agent knows only when it is", async () => {
  await page.viewport(1200, 800);
  const known = await render(ResultCard, {
    id: "result",
    item: result(chart(true)),
    selected: false,
    onaction: () => {},
  });
  await expect.poll(() => known.container.querySelector(".chart.compact svg")).not.toBeNull();
  expect(known.container.querySelector(".chart details")).toBeNull();
  expect(known.container.querySelector(".chart .legend")).toBeNull();
  await expect.element(known.getByText("From what the agent knows")).toBeVisible();
  expect(known.container.textContent?.match(/From what the agent knows/gu)).toHaveLength(1);
  await known.unmount();
  const cited = await render(ResultCard, {
    id: "result",
    item: result(chart(false)),
    selected: false,
    onaction: () => {},
  });
  await expect.poll(() => cited.container.querySelector(".chart.compact svg")).not.toBeNull();
  expect(cited.container.textContent).not.toContain("From what the agent knows");
  await cited.unmount();
});
