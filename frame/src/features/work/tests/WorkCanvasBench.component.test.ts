import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import BoardCanvas from "./BoardCanvas.svelte";
import type { BoardScene } from "./board-fixtures";

// Real works exported from the QA profile (see WorkBandLook); without them there is nothing to measure.
const LOOK = "/node_modules/.work-look";
vi.mock("$domain/resources", async (original) => ({
  ...(await original<typeof import("$domain/resources")>()),
  pageFrameUrl: (attempt: string, step: string) => `${LOOK}/frames/${attempt}-${step}.png`,
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ faviconProbe: async () => true });
});

type Raw = {
  snapshot: BoardScene["snapshot"];
  objectives: Record<string, WorkRuntimeProjection>;
  pages: BoardScene["pages"];
};

/** Several real works on one canvas: their elements, runs and pages together. */
async function merged(names: readonly string[]): Promise<BoardScene | null> {
  const raws: Raw[] = [];
  for (const name of names) {
    const response = await fetch(`${LOOK}/${name}/scene.json`);
    if (!response.ok) return null;
    raws.push((await response.json()) as Raw);
  }
  const [first] = raws;
  if (!first) return null;
  return {
    name: "bench",
    snapshot: { ...first.snapshot, elements: raws.flatMap((raw) => raw.snapshot.elements) },
    objectives: new Map(raws.flatMap((raw) => Object.entries(raw.objectives))),
    pictures: new Map(),
    pages: raws.flatMap((raw) => raw.pages),
  };
}

/**
 * While the canvas pans on every frame, as a trackpad would: each frame's
 * interval, and the main-thread work a pan step costs (the flow's update,
 * style and layout, forced before the frame is drawn).
 */
async function pan(
  target: Element,
  frames: number,
  zoom: number,
): Promise<{ intervals: number[]; work: number[] }> {
  const intervals: number[] = [];
  const work: number[] = [];
  const probe = target
    .closest(".svelte-flow")!
    .querySelector<HTMLElement>(".svelte-flow__viewport")!;
  let last = performance.now();
  for (let frame = 0; frame < frames; frame++) {
    const start = performance.now();
    target.dispatchEvent(
      new WheelEvent("wheel", {
        // A slow figure of eight over the works: every frame moves, the view stays on them.
        deltaX: (Math.cos((frame / 60) * Math.PI) * 30) / zoom,
        deltaY: (Math.sin((frame / 30) * Math.PI) * 20) / zoom,
        bubbles: true,
        cancelable: true,
      }),
    );
    await Promise.resolve();
    void probe.getBoundingClientRect();
    for (const node of probe.querySelectorAll<HTMLElement>(".svelte-flow__node"))
      void node.offsetWidth;
    work.push(performance.now() - start);
    await new Promise((done) => requestAnimationFrame(() => done(null)));
    const now = performance.now();
    intervals.push(now - last);
    last = now;
  }
  return { intervals, work };
}

const stats = (times: readonly number[]) => {
  const sorted = [...times].sort((a, b) => a - b);
  const at = (q: number) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))]!;
  return {
    mean: +(times.reduce((sum, time) => sum + time, 0) / times.length).toFixed(2),
    p50: +at(0.5).toFixed(2),
    p95: +at(0.95).toFixed(2),
    max: +at(1).toFixed(2),
  };
};

test.each([1, 0.5])("panning a 50-object work at %sx", async (zoom) => {
  await page.viewport(1440, 900);
  const scene = await merged(["jobs", "learning", "db", "trip", "saas"]);
  if (!scene) return;
  const screen = await render(BoardCanvas, { scene, viewport: { x: 0, y: 0, zoom } });
  screen.container.style.width = "1440px";
  screen.container.style.height = "900px";
  await new Promise((done) => setTimeout(done, 1500));
  const nodes = screen.container.querySelectorAll(".svelte-flow__node").length;
  const pane = screen.container.querySelector(".svelte-flow__pane")!;
  const { intervals, work } = await pan(pane, 240, zoom);
  const objects = [
    ...new Set(
      [...scene.objectives.values()].flatMap((projection) =>
        projection.executions.flatMap((execution) => execution.artifacts.map((a) => a.id)),
      ),
    ),
  ].length;
  const result = { zoom, objects, drawn: nodes, frame: stats(intervals), work: stats(work) };
  // The numbers are the report; the bound only keeps a regression visible.
  console.warn("work-canvas-bench", JSON.stringify(result));
  expect(result.work.p50).toBeLessThan(8);
  await page.screenshot({ path: `../../../../../target/work-band/bench-${zoom}.png` });
  await screen.unmount();
});
