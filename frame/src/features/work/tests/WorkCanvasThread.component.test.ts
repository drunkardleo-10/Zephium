import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import WorkCanvas from "../components/WorkCanvas.svelte";
import { environmentItems } from "../lib/project-environment";
import { environmentRequests, environmentStages } from "../lib/project-environment-thread";
import { projection, snapshot } from "./environment-fixtures";
import type { CanvasItem, CanvasLink } from "../lib/canvas-model";

const messages = [
  "Compare quiet keyboards",
  "Show me the quietest one",
  "And the wireless ones",
] as const;

test("three messages stand as three request cards, and one bad card never blanks the canvas", async () => {
  await page.viewport(1200, 800);
  const state = structuredClone(projection);
  const first = state.executions[0]!;
  first.spec.request = messages[0];
  state.executions = messages.slice(1).map((request, index) => {
    const next = structuredClone(first);
    next.id = `continuation-${index + 1}`;
    next.spec.request = request;
    return next;
  });
  state.executions.unshift(first);
  state.work.objective = messages.at(-1)!;
  const scene = {
    ...snapshot,
    elements: snapshot.elements.slice(0, 1),
    view: {
      ...snapshot.view,
      placements: [{ element: "objective-card", x: 0, y: 0, width: 320, height: 150 }],
    },
  };
  const objectives = new Map([["objective", state]]);
  const stages = environmentStages(scene, objectives);
  const requests = environmentRequests(stages);
  const items: CanvasItem[] = [
    ...environmentItems(scene, [], [], objectives),
    ...requests.items,
    // Everything a producer can get wrong, in one card and one edge.
    { id: "", title: "x".repeat(900), kind: "Finding", detail: "y".repeat(5000), status: "" },
  ];
  const links: CanvasLink[] = [
    ...requests.links,
    { id: "dangling", source: "objective-card", target: "gone", kind: "reference" },
  ];
  const screen = await render(WorkCanvas, {
    items,
    links,
    authoritative: new Set(["objective-card"]),
    initialView: {
      positions: { "objective-card": { x: 0, y: 0 }, ...requests.positions },
      viewport: { x: 24, y: 24, zoom: 0.4 },
    },
    oninspect: vi.fn(),
  });
  screen.container.style.width = "1200px";
  screen.container.style.height = "800px";
  for (const message of messages)
    await expect.element(screen.getByText(message, { exact: true })).toBeVisible();
  expect(screen.container.textContent).not.toContain("cannot be displayed");
  const cards = [...screen.container.querySelectorAll(".request")].map((card) =>
    card.getBoundingClientRect(),
  );
  expect(cards).toHaveLength(3);
  for (const [index, card] of cards.slice(1).entries())
    expect(card.top).toBeGreaterThan(cards[index]!.bottom);
  await screen.unmount();
});
