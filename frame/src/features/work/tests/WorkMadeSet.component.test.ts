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
import { environmentClusters, environmentDiagrams, resultHeads } from "../lib/project-environment";
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
  const lanes = environmentClusters(stages, resultHeads(snapshot, objectives, stages));
  // The diagram draws no cover: its area stands for it.
  return {
    items: diagrams.items,
    links: [...lanes.links, ...diagrams.links],
    clusters: [...diagrams.clusters, ...lanes.clusters],
    positions: { ...environmentRequests(stages).positions, ...diagrams.positions },
    stages,
  };
}

test("a diagram result is its area of parts alone, layered left to right, joined, and captioned by its title", async () => {
  await page.viewport(1400, 900);
  const { items, links, clusters, positions, stages } = scene(DIAGRAM);
  expect(stages[0]!.layout.positions["result-card"]).toBeUndefined();
  const opened: string[] = [];
  const screen = await render(WorkCanvas, {
    items,
    links,
    clusters,
    authoritative: new Set(["result-card"]),
    initialView: { positions, viewport: { x: 0, y: 40, zoom: 0.8 } },
    oninspect: () => {},
    onopen: (id: string) => opened.push(id),
  });
  screen.container.style.width = "1400px";
  screen.container.style.height = "900px";
  const area = () =>
    screen.container.querySelector(
      '.svelte-flow__node[data-id="group:objective-card:diagram:result-card"]',
    );
  await expect.poll(() => area()?.querySelector(".title")?.textContent).toBe("Checkout system");
  expect(area()!.querySelector(".take")?.textContent).toBe("4 parts");
  expect(screen.container.querySelector('[data-id="result-card"]')).toBeNull();
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
  // The title opens the diagram; the count takes it whole.
  await screen.getByRole("button", { name: "Checkout system" }).click();
  expect(opened).toEqual(["result-card"]);
  await screen.getByTitle("Select the whole diagram").click();
  await expect
    .poll(() => screen.container.querySelectorAll(".svelte-flow__node.selected").length)
    .toBe(4);
  await screen.unmount();
});

/** The person's architecture: six layers, fourteen parts, nineteen named flows. */
const TRIP: WorkArtifactDataV1 = {
  kind: "diagram",
  layers: [
    { id: "clients", name: "Clients" },
    { id: "edge", name: "Edge" },
    { id: "gateway", name: "Gateway" },
    { id: "services", name: "Services" },
    { id: "data", name: "Data" },
    { id: "external", name: "External" },
  ],
  nodes: [
    { id: "web", name: "Web app", kind: "client", layer: "clients" },
    { id: "mobile", name: "Mobile app", kind: "client", layer: "clients" },
    { id: "cdn", name: "CDN and static hosting", kind: "edge", layer: "edge" },
    { id: "lb", name: "Load balancer", kind: "edge", layer: "edge" },
    { id: "api", name: "API gateway", kind: "gateway", layer: "gateway" },
    { id: "auth", name: "Identity and authentication", kind: "gateway", layer: "gateway" },
    { id: "trips", name: "Trip service", kind: "service", layer: "services" },
    { id: "booking", name: "Booking service", kind: "service", layer: "services" },
    { id: "worker", name: "Background workers and schedulers", kind: "worker", layer: "services" },
    { id: "notify", name: "Notification service", kind: "service", layer: "services" },
    { id: "pg", name: "PostgreSQL", kind: "store", layer: "data" },
    { id: "redis", name: "Redis", kind: "cache", layer: "data" },
    { id: "storage", name: "Object storage", kind: "storage", layer: "data" },
    { id: "payments", name: "Payment provider", kind: "external", layer: "external" },
  ],
  edges: [
    { from: "web", to: "cdn", label: "static assets" },
    { from: "mobile", to: "lb", label: "HTTPS" },
    { from: "web", to: "lb", label: "HTTPS" },
    { from: "cdn", to: "api", label: "API requests" },
    { from: "lb", to: "api", label: "API requests" },
    { from: "api", to: "auth", label: "token checks" },
    { from: "api", to: "trips", label: "authenticated requests" },
    { from: "api", to: "booking", label: "authenticated requests" },
    { from: "trips", to: "pg", label: "tenant and user checks" },
    { from: "booking", to: "pg", label: "reads and writes" },
    { from: "trips", to: "redis", label: "cache" },
    { from: "booking", to: "payments", label: "charges" },
    { from: "worker", to: "pg", label: "jobs" },
    { from: "booking", to: "worker", label: "enqueue" },
    { from: "worker", to: "notify", label: "events" },
    { from: "notify", to: "storage", label: "templates" },
    { from: "trips", to: "storage", label: "uploads" },
    { from: "auth", to: "pg", label: "sessions" },
    { from: "payments", to: "booking", label: "webhooks" },
  ],
};

test("the person's architecture fits its group, and no flow's name sits on a part", async () => {
  await page.viewport(1600, 1000);
  const { items, links, clusters, positions } = scene(TRIP);
  const screen = await render(WorkCanvas, {
    items,
    links,
    clusters,
    authoritative: new Set(["result-card"]),
    initialView: { positions, viewport: { x: 0, y: 40, zoom: 0.5 } },
    virtualizeFrom: 1000,
    oninspect: () => {},
  });
  screen.container.style.width = "1600px";
  screen.container.style.height = "1000px";
  await expect.poll(() => document.querySelectorAll(".work-edge-label").length).toBe(19);
  const rect = (element: Element) => element.getBoundingClientRect();
  const cards = [
    ...screen.container.querySelectorAll('.svelte-flow__node[data-id^="diagram:result-card:"]'),
  ].map(rect);
  expect(cards).toHaveLength(14);
  const overlaps = (a: DOMRect, b: DOMRect) =>
    a.left < b.right - 0.5 &&
    b.left < a.right - 0.5 &&
    a.top < b.bottom - 0.5 &&
    b.top < a.bottom - 0.5;
  const plates = [...document.querySelectorAll(".work-edge-label")];
  for (const plate of plates)
    for (const card of cards)
      expect(overlaps(rect(plate), card), `"${plate.textContent}" over a part`).toBe(false);
  // The area holds every part and plate, and the Made group holds the area.
  const node = (id: string) =>
    rect(screen.container.querySelector(`.svelte-flow__node[data-id="${id}"]`)!);
  const area = node("group:objective-card:diagram:result-card");
  const made = node("group:objective-card:made");
  for (const inner of [...cards, ...plates.map(rect)]) {
    expect(inner.left).toBeGreaterThanOrEqual(area.left);
    expect(inner.right).toBeLessThanOrEqual(area.right + 0.5);
    expect(inner.bottom).toBeLessThanOrEqual(area.bottom + 0.5);
  }
  expect(area.left).toBeGreaterThan(made.left);
  expect(area.right).toBeLessThan(made.right);
  expect(area.bottom).toBeLessThan(made.bottom);
  // A long name wraps to a second line instead of clipping.
  const name = screen.container.querySelector<HTMLElement>(
    '[data-id="diagram:result-card:worker"] .name',
  )!;
  expect(name.textContent).toBe("Background workers and schedulers");
  expect(name.scrollHeight).toBeLessThanOrEqual(name.clientHeight + 1);
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

const result = (data: WorkArtifactDataV1, general_knowledge?: boolean): CanvasItem => {
  const state = structuredClone(projection);
  const execution = state.executions[0]!;
  execution.artifacts[0]!.data = data;
  if (general_knowledge !== undefined)
    execution.artifacts[0]!.general_knowledge = general_knowledge;
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
  // Marked on the chart and not on the result: still one caption, the card's own.
  for (const whole of [true, false]) {
    const both = await render(ResultCard, {
      id: "result",
      item: result(chart(true), whole),
      selected: false,
      onaction: () => {},
    });
    await expect.poll(() => both.container.querySelector(".chart.compact svg")).not.toBeNull();
    expect(both.container.textContent?.match(/From what the agent knows/gu)).toHaveLength(1);
    expect(both.container.querySelector(".chart .caption")).toBeNull();
    await both.unmount();
  }
});

test("a code card numbers its first fourteen lines, bars a note and says what is past them", async () => {
  await page.viewport(1200, 800);
  const text = Array.from({ length: 20 }, (_, index) => `let x${index + 1} = ${index + 1};`).join(
    "\n",
  );
  const screen = await render(ResultCard, {
    id: "result",
    item: result({
      kind: "code",
      language: "rust",
      text,
      notes: [{ from: 2, to: 3, text: "The second and third bindings" }],
    }),
    selected: false,
    onaction: () => {},
  });
  await expect.element(screen.getByText("Code · rust")).toBeVisible();
  await expect.poll(() => screen.container.querySelectorAll(".row").length).toBe(14);
  expect(screen.container.querySelector(".row:last-child .n")?.textContent).toBe("14");
  expect(screen.container.querySelectorAll(".row.noted")).toHaveLength(2);
  await expect.element(screen.getByText("+6 lines")).toBeVisible();
  await expect.element(screen.getByText("1 note")).toBeVisible();
  await page.elementLocator(screen.container.querySelectorAll(".row")[2]!).hover();
  await expect.element(screen.getByRole("note")).toHaveTextContent("The second and third bindings");
  await screen.unmount();
});
