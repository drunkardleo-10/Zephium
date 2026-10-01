import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import type { MediaAssetV1 } from "$domain/resources";
import BoardCanvas from "./BoardCanvas.svelte";
import type { BoardScene } from "./board-fixtures";
import { elementPictures } from "../lib/project-environment-board";

/**
 * An all-day work: the QA profile's real works (see WorkBandLook) stacked on
 * one canvas until it holds about thirty runs, with their picks, sheets,
 * diagrams and page windows. It measures what the canvas costs to open, pan
 * and zoom, what its pictures hold decoded, and what moves at rest.
 */
const LOOK = "/node_modules/.work-look";
/** Where each copied frame and picture really lies, by the ids the copy gave it. */
const frames = new Map<string, string>();
const pictures = new Map<string, string>();
vi.mock("$domain/resources", async (original) => ({
  ...(await original<typeof import("$domain/resources")>()),
  pageFrameUrl: (attempt: string, step: string) => frames.get(`${attempt}-${step}`) ?? null,
  mediaUrl: (_profile: string, digest: string) => pictures.get(digest) ?? "",
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ faviconProbe: async () => true });
});

type Raw = {
  snapshot: BoardScene["snapshot"];
  objectives: Record<string, WorkRuntimeProjection>;
  pages: BoardScene["pages"];
  media?: Record<string, MediaAssetV1>;
};
const EXTENSION: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/webp": "webp",
  "image/gif": "gif",
};
const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const ULID = /\b[0-9A-HJKMNP-TV-Z]{26}\b/gu;

/** One copy of a work with every id its own: the last character of each ULID set by the copy. */
function copy(raw: Raw, index: number, name: string): Raw {
  const mark = CROCKFORD[index % 32]!;
  const renamed = (text: string) => text.replace(ULID, (id) => `${id.slice(0, 25)}${mark}`);
  const out = JSON.parse(renamed(JSON.stringify(raw))) as Raw;
  raw.pages.forEach((original, at) => {
    const page = out.pages[at]!;
    frames.set(
      `${page.attempt}-${page.step}`,
      `${LOOK}/${name}/frames/${original.attempt}-${original.step}.png`,
    );
  });
  for (const asset of Object.values(raw.media ?? {}))
    pictures.set(
      asset.digest,
      `${LOOK}/${name}/media/${asset.digest}.${EXTENSION[asset.mime] ?? "png"}`,
    );
  return out;
}

const WORKS = [
  "trip3",
  "compare",
  "compilers",
  "lunios",
  "day",
  "yctrip",
  "aisaas",
  "lego",
  "learning",
  "jobs",
];

async function allDay(runs: number): Promise<{ scene: BoardScene; runs: number } | null> {
  const loaded: [string, Raw][] = [];
  for (const name of WORKS) {
    const response = await fetch(`${LOOK}/${name}/scene.json`);
    if (response.ok) loaded.push([name, (await response.json()) as Raw]);
  }
  if (!loaded.length) return null;
  const parts: Raw[] = [];
  let count = 0;
  for (let index = 0; count < runs; index++) {
    const [name, raw] = loaded[index % loaded.length]!;
    const one = copy(raw, Math.floor(index / loaded.length), name);
    parts.push(one);
    count += Object.values(one.objectives).reduce(
      (sum, projection) => sum + new Set(projection.executions.map((e) => e.spec.request)).size,
      0,
    );
  }
  const first = parts[0]!;
  const snapshot = {
    ...first.snapshot,
    elements: parts.flatMap((part) => part.snapshot.elements),
    relations: parts.flatMap((part) => part.snapshot.relations ?? []),
    view: { ...first.snapshot.view, placements: [] },
  };
  const media = new Map(parts.flatMap((part) => Object.entries(part.media ?? {})));
  return {
    runs: count,
    scene: {
      name: "all-day",
      snapshot,
      objectives: new Map(parts.flatMap((part) => Object.entries(part.objectives))),
      pictures: elementPictures(snapshot, media),
      pages: parts.flatMap((part) => part.pages),
      media,
    },
  };
}

const frame = () => new Promise((done) => requestAnimationFrame(() => done(null)));
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

/** Main-thread work a gesture step costs: the flow's update, then style and layout forced before the frame. */
async function gesture(
  target: Element,
  steps: number,
  event: (step: number) => WheelEventInit,
): Promise<number[]> {
  const viewport = target
    .closest(".svelte-flow")!
    .querySelector<HTMLElement>(".svelte-flow__viewport")!;
  const work: number[] = [];
  for (let step = 0; step < steps; step++) {
    const start = performance.now();
    target.dispatchEvent(
      new WheelEvent("wheel", { bubbles: true, cancelable: true, ...event(step) }),
    );
    await Promise.resolve();
    void viewport.getBoundingClientRect();
    for (const node of viewport.querySelectorAll<HTMLElement>(".svelte-flow__node"))
      void node.offsetWidth;
    work.push(performance.now() - start);
    await frame();
  }
  return work;
}

/** What the canvas's pictures hold decoded now, as bytes of RGBA, and how many are drawn. */
function decoded(root: Element) {
  let bytes = 0;
  let images = 0;
  for (const image of root.querySelectorAll("img")) {
    if (!image.complete || !image.naturalWidth) continue;
    images += 1;
    bytes += image.naturalWidth * image.naturalHeight * 4;
  }
  for (const canvas of root.querySelectorAll("canvas")) bytes += canvas.width * canvas.height * 4;
  return { images, mb: +(bytes / 1048576).toFixed(1) };
}

test("an all-day work: open, pan, zoom, pictures and rest", { timeout: 90_000 }, async () => {
  await page.viewport(1440, 900);
  const built = await allDay(30);
  if (!built) return;
  const { environmentStages } = await import("../lib/project-environment-board");
  const laid = performance.now();
  const stages = environmentStages(built.scene.snapshot, built.scene.objectives, {
    recorded: () => built.scene.pages,
  });
  const layout = +(performance.now() - laid).toFixed(1);
  const projection = await import("../lib/project-environment");
  const thread = await import("../lib/project-environment-thread");
  const timed = <T>(run: () => T): number => {
    const start = performance.now();
    run();
    return +(performance.now() - start).toFixed(1);
  };
  const projections = {
    items: timed(() =>
      projection.environmentItems(
        built.scene.snapshot,
        [],
        [],
        built.scene.objectives,
        built.scene.media,
      ),
    ),
    boards: timed(() => projection.environmentBoards(stages)),
    parts: timed(() =>
      projection.environmentParts(built.scene.objectives, stages, () => built.scene.pages),
    ),
    requests: timed(() => thread.environmentRequests(stages)),
    reconcile: await (async () => {
      const model = await import("../lib/canvas-model");
      const list = [
        ...projection.environmentItems(
          built.scene.snapshot,
          [],
          [],
          built.scene.objectives,
          built.scene.media,
        ),
        ...thread.environmentRequests(stages).items,
        ...projection.environmentBoards(stages),
        ...projection.environmentParts(built.scene.objectives, stages, () => built.scene.pages),
        ...projection.environmentInputs(stages),
        ...projection.environmentSources(stages),
      ];
      const start = performance.now();
      const nodes = model.reconcileNodes([], list, thread.environmentRequests(stages).positions);
      const once = performance.now() - start;
      const again = performance.now();
      model.reconcileNodes(nodes, list, thread.environmentRequests(stages).positions);
      return {
        count: list.length,
        first: +once.toFixed(1),
        again: +(performance.now() - again).toFixed(1),
      };
    })(),
  };
  // The first open fetches the lazy renderers from the dev server; the second is the canvas's own cost.
  const warm = await render(BoardCanvas, {
    scene: built.scene,
    viewport: { x: 120, y: 72, zoom: 1 },
  });
  await new Promise((done) => setTimeout(done, 1500));
  await warm.unmount();
  warm.container.remove();
  const opened = performance.now();
  const screen = await render(BoardCanvas, {
    scene: built.scene,
    viewport: { x: 120, y: 72, zoom: 1 },
  });
  const mounted = +(performance.now() - opened).toFixed(0);
  screen.container.style.width = "1440px";
  screen.container.style.height = "900px";
  // Open: until what the view draws has stopped changing for ten frames.
  let last = -1;
  let still = 0;
  let firstPaint = 0;
  const gaps: number[] = [];
  let before = performance.now();
  for (let tick = 0; tick < 600 && still < 10; tick++) {
    await frame();
    const now = performance.now();
    gaps.push(now - before);
    before = now;
    const nodes = screen.container.querySelectorAll(".svelte-flow__node").length;
    if (nodes && !firstPaint) firstPaint = performance.now() - opened;
    still = nodes === last ? still + 1 : 0;
    last = nodes;
  }
  const open = {
    firstPaint: +firstPaint.toFixed(0),
    settled: +(performance.now() - opened - 160).toFixed(0),
    // Main-thread time the open blocked frames for: every frame's excess over one display frame.
    blocking: +gaps.reduce((sum, gap) => sum + Math.max(0, gap - 16.7), 0).toFixed(0),
    longest: +Math.max(...gaps).toFixed(0),
    frames: gaps.length,
  };
  const items = built.scene.snapshot.elements.length;
  const drawn100 = screen.container.querySelectorAll(".svelte-flow__node").length;
  const pictures100 = decoded(screen.container);
  const pane = screen.container.querySelector(".svelte-flow__pane")!;
  const pan100 = await gesture(pane, 180, (step) => ({
    deltaX: Math.cos((step / 60) * Math.PI) * 30,
    deltaY: Math.sin((step / 30) * Math.PI) * 24 + 6,
  }));
  // Zoom out to a survey of the whole day and back, as a pinch does.
  const zoomOut = await gesture(pane, 90, () => ({
    deltaY: 6,
    ctrlKey: true,
    clientX: 720,
    clientY: 450,
  }));
  const scale = () =>
    +Number(
      /scale\(([\d.]+)\)/u.exec(
        screen.container.querySelector<HTMLElement>(".svelte-flow__viewport")!.style.transform,
      )?.[1] ?? 0,
    ).toFixed(2);
  const zoomedOutTo = scale();
  const zoomIn = await gesture(pane, 90, () => ({
    deltaY: -6,
    ctrlKey: true,
    clientX: 720,
    clientY: 450,
  }));
  await new Promise((done) => setTimeout(done, 1500));
  const moving = document
    .getAnimations()
    .filter((animation) => animation.playState === "running").length;
  const result = {
    runs: built.runs,
    items,
    open,
    layout: { ms: layout, stages: stages.length, projections, mounted },
    zoomedOutTo,
    drawn: { at100: drawn100 },
    decoded: { at100: pictures100 },
    work: {
      pan100: stats(pan100),
      zoomOut: stats(zoomOut),
      zoomIn: stats(zoomIn),
    },
    animationsAtRest: moving,
  };
  await page.screenshot({ path: "../../../../../target/work-band/all-day-rest.png" });
  await screen.unmount();
  screen.container.remove();
  // Surveying the day: the same work opened at 50% and at 25%, then panned down it.
  const survey: Record<string, unknown> = {};
  for (const zoom of [0.5, 0.25]) {
    const view = await render(BoardCanvas, {
      scene: built.scene,
      viewport: { x: 120, y: 72, zoom },
    });
    view.container.style.width = "1440px";
    view.container.style.height = "900px";
    await new Promise((done) => setTimeout(done, 1500));
    const drawn = view.container.querySelectorAll(".svelte-flow__node").length;
    const held = decoded(view.container);
    await page.screenshot({ path: `../../../../../target/work-band/all-day-${zoom * 100}.png` });
    const at = view.container.querySelector(".svelte-flow__pane")!;
    const panned = await gesture(at, 150, (step) => ({
      deltaX: Math.cos((step / 60) * Math.PI) * 20,
      deltaY: 12,
    }));
    await new Promise((done) => setTimeout(done, 600));
    survey[`at${zoom * 100}`] = {
      drawn,
      decoded: held,
      pan: stats(panned),
      afterPan: {
        drawn: view.container.querySelectorAll(".svelte-flow__node").length,
        decoded: decoded(view.container),
      },
    };
    await view.unmount();
    view.container.remove();
  }
  console.warn("work-all-day", JSON.stringify({ ...result, survey }));
  expect(moving).toBe(0);
  expect(built.runs).toBeGreaterThanOrEqual(30);
});
