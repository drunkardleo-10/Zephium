import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import WorkCanvas from "../components/WorkCanvas.svelte";
import {
  environmentClusters,
  environmentItems,
  environmentSteps,
} from "../lib/project-environment";
import { environmentStages } from "../lib/project-environment-thread";
import { planScene } from "./environment-fixtures";

test("a plan's four steps stand as four cards beside the result, which keeps its summary", async () => {
  await page.viewport(1400, 900);
  const { scene, objectives } = planScene();
  const stages = environmentStages(scene, objectives);
  const steps = environmentSteps(scene, objectives, stages);
  const { clusters, links } = environmentClusters(stages);
  const screen = await render(WorkCanvas, {
    items: [...environmentItems(scene, [], [], objectives), ...steps.items],
    links: [...links, ...steps.links],
    clusters,
    authoritative: new Set(["objective-card", "plan-card"]),
    initialView: {
      positions: {
        ...stages[0]!.layout.positions,
        ...steps.positions,
        "objective-card": { x: 0, y: 0 },
      },
      viewport: { x: 16, y: 16, zoom: 0.6 },
    },
    oninspect: vi.fn(),
  });
  screen.container.style.width = "1400px";
  screen.container.style.height = "900px";
  const cards = () => [...screen.container.querySelectorAll<HTMLElement>(".step")];
  await expect.poll(() => cards().length).toBe(4);
  expect(cards().map((card) => card.dataset.icon)).toEqual(["dates", "entry", "stay", "flight"]);
  expect(cards().map((card) => card.querySelector(".index")?.textContent)).toEqual([
    "1",
    "2",
    "3",
    "4",
  ]);
  await expect
    .element(screen.getByText("Check your ESTA eligibility and official entry requirements."))
    .toBeVisible();
  // The result keeps its summary and sections; its steps are not repeated on it.
  const result = screen.container.querySelector<HTMLElement>(".artifact-body")!;
  expect(result.textContent).toContain("Planning note: dates and budget");
  expect(result.textContent).toContain("Entry and arrival");
  expect(result.textContent).not.toContain("ESTA eligibility");
  // Steps sit to the right of the result, two across.
  const box = (element: Element) => element.getBoundingClientRect();
  expect(box(cards()[0]!).left).toBeGreaterThan(box(result).right);
  expect(box(cards()[1]!).left).toBeGreaterThan(box(cards()[0]!).right);
  expect(box(cards()[2]!).top).toBeGreaterThan(box(cards()[0]!).bottom);
  await expect.element(screen.getByText("4 steps", { exact: true })).toBeInTheDocument();
  await screen.unmount();
});
