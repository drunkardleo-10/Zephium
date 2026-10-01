import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import type { MediaAssetV1 } from "$domain/resources";
import BoardCanvas from "./BoardCanvas.svelte";
import type { BoardScene } from "./board-fixtures";
import { elementPictures, fetchedPictures } from "../lib/project-environment-board";

// Today's heaviest real works, exported read-only (see WorkReleaseLook); run with VITE_PAN=1.
const LOOK = "/node_modules/.work-look";
let shown = "";
vi.mock("$domain/resources", async (original) => ({
  ...(await original<typeof import("$domain/resources")>()),
  pageFrameUrl: (attempt: string, step: string) => `${LOOK}/${shown}/frames/${attempt}-${step}.png`,
  mediaUrl: (_profile: string, digest: string) => `${LOOK}/${shown}/media/${digest}.png`,
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ faviconProbe: async () => true });
});

async function scene(name: string): Promise<BoardScene | null> {
  const response = await fetch(`${LOOK}/${name}/scene.json`);
  if (!response.ok) return null;
  const raw = (await response.json()) as {
    snapshot: BoardScene["snapshot"];
    objectives: Record<string, WorkRuntimeProjection>;
    pages: BoardScene["pages"];
    media?: Record<string, MediaAssetV1>;
  };
  const media = new Map(Object.entries(raw.media ?? {}));
  return {
    name,
    snapshot: raw.snapshot,
    objectives: new Map(Object.entries(raw.objectives)),
    pictures: elementPictures(raw.snapshot, media),
    pages: raw.pages,
    media,
  };
}

const stats = (times: readonly number[]) => {
  const sorted = [...times].sort((a, b) => a - b);
  const at = (q: number) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))]!;
  return {
    frames: times.length,
    mean: +(times.reduce((sum, time) => sum + time, 0) / times.length).toFixed(1),
    p95: +at(0.95).toFixed(1),
    max: +at(1).toFixed(1),
    long: times.filter((time) => time > 25).length,
    at: times.flatMap((time, index) => (time > 25 ? [index] : [])).join(" "),
  };
};

/** Every frame a trackpad step: a pan (wheel) or a pinch (ctrl + wheel), frame intervals recorded. */
async function drive(target: Element, frames: number, pinch: boolean): Promise<number[]> {
  const intervals: number[] = [];
  // Start from rest, as a person does, their pointer arriving first: the first frames count.
  await new Promise((done) => setTimeout(done, 800));
  target.dispatchEvent(
    new PointerEvent("pointermove", { bubbles: true, clientX: 700, clientY: 440 }),
  );
  await new Promise((done) => setTimeout(done, 200));
  let last = performance.now();
  for (let frame = 0; frame < frames; frame++) {
    target.dispatchEvent(
      new WheelEvent("wheel", {
        deltaX: pinch ? 0 : Math.cos((frame / 45) * Math.PI) * 24,
        deltaY: pinch
          ? Math.sin((frame / 30) * Math.PI) * 6
          : Math.sin((frame / 30) * Math.PI) * 16,
        ctrlKey: pinch,
        bubbles: true,
        cancelable: true,
        clientX: 720,
        clientY: 450,
      }),
    );
    await new Promise((done) => requestAnimationFrame(() => done(null)));
    const now = performance.now();
    intervals.push(now - last);
    last = now;
  }
  return intervals;
}

const WORKS = ["trip5", "blueprint", "lego5", "research"];
const on = !!import.meta.env.VITE_PAN;

test.skipIf(!on).each(WORKS)(
  "%s pans and pinches in steady frames",
  async (name) => {
    await page.viewport(1440, 900);
    const found = await scene(name);
    if (!found) return;
    shown = name;
    const screen = await render(BoardCanvas, {
      scene: { ...found, ...(found.media ? {} : {}) },
      viewport: { x: 80, y: 40, zoom: 0.7 },
    });
    void fetchedPictures;
    screen.container.style.width = "1440px";
    screen.container.style.height = "900px";
    if (import.meta.env.VITE_PAN === "will")
      document.head.insertAdjacentHTML(
        "beforeend",
        import.meta.env.VITE_BG
          ? "<style id='bench-will'>.svelte-flow__viewport{will-change:transform}.svelte-flow__background{display:none}</style>"
          : "<style id='bench-will'>.svelte-flow__viewport{will-change:transform}</style>",
      );
    await new Promise((done) => setTimeout(done, 2500));
    const pane = screen.container.querySelector(".svelte-flow__pane")!;
    const nodes = screen.container.querySelectorAll(".svelte-flow__node").length;
    const panned = stats(await drive(pane, 180, false));
    const pinched = stats(await drive(pane, 120, true));
    console.warn("PAN", JSON.stringify({ name, nodes, pan: panned, pinch: pinched }));
    document.getElementById("bench-will")?.remove();
    await screen.unmount();
  },
  60_000,
);
