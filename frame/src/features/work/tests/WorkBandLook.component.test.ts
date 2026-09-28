import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import BoardCanvas from "./BoardCanvas.svelte";
import type { BoardScene } from "./board-fixtures";

// Real runs exported read-only from the QA profile into node_modules/.work-look;
// without them there is nothing to look at and the test only renders nothing.
const LOOK = "/node_modules/.work-look";
let shown = "";
vi.mock("$domain/resources", async (original) => ({
  ...(await original<typeof import("$domain/resources")>()),
  pageFrameUrl: (attempt: string, step: string) => `${LOOK}/${shown}/frames/${attempt}-${step}.png`,
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ faviconProbe: async () => true });
});

const shots = "../../../../../target/work-band";

async function scene(name: string): Promise<BoardScene | null> {
  const response = await fetch(`${LOOK}/${name}/scene.json`);
  if (!response.ok) return null;
  const raw = (await response.json()) as {
    snapshot: BoardScene["snapshot"];
    objectives: Record<string, WorkRuntimeProjection>;
    pages: BoardScene["pages"];
  };
  return {
    name,
    snapshot: raw.snapshot,
    objectives: new Map(Object.entries(raw.objectives)),
    pictures: new Map(),
    pages: raw.pages,
  };
}

test.each(["trip", "saas", "db", "jobs", "dinner"])("%s laid out as a band", async (name) => {
  await page.viewport(1440, 900);
  const loaded = await scene(name);
  if (!loaded) return;
  shown = name;
  const errors: string[] = [];
  const listen = (event: ErrorEvent) => errors.push(event.message);
  window.addEventListener("error", listen);
  const screen = await render(BoardCanvas, {
    scene: loaded,
    viewport: { x: 48, y: 72, zoom: 0.72 },
  });
  screen.container.style.width = "1440px";
  screen.container.style.height = "900px";
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await new Promise((done) => setTimeout(done, 900));
    await page.screenshot({ path: `${shots}/${name}-${theme}.png` });
  }
  document.documentElement.dataset.theme = "dark";
  window.removeEventListener("error", listen);
  if (errors.length) console.error(name, errors.join(" | "));
  await screen.unmount();
});
