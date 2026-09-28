import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import BoardCanvas from "./BoardCanvas.svelte";
import type { BoardScene } from "./board-fixtures";
import { askingTrip, leadTrip } from "./lead-look";

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

/** The same work caught mid-run: its last execution still reading its last page. */
function midRun(loaded: BoardScene): BoardScene {
  const objectives = new Map(
    [...loaded.objectives].map(([id, projection]) => {
      const copy = structuredClone(projection);
      const execution = copy.executions.at(-1);
      const steps = execution?.steps ?? [];
      const last = steps.findLastIndex((step) => step.kind.kind === "read");
      if (execution && last >= 0) {
        execution.status = "running";
        execution.steps = steps.slice(0, last + 1);
        execution.steps[last] = { ...execution.steps[last]!, status: "running" };
        execution.artifacts = execution.artifacts.filter((artifact) =>
          execution.steps!.some((step) => step.artifacts?.includes(artifact.id)),
        );
      }
      return [id, copy] as const;
    }),
  );
  return { ...loaded, objectives };
}

const WORKS = [
  "trip",
  "saas",
  "db",
  "jobs",
  "slack",
  "dinner",
  "learning",
  "browser",
  "lego",
  "aisaas",
];
const LOOKS = [
  ...WORKS.flatMap(
    (name) =>
      [
        [name, 100],
        [name, 50],
      ] as const,
  ),
  ["trip-live", 100],
  ["learning-live", 100],
  ["trip-lead", 100],
  ["trip-lead", 50],
  ["trip-leadlive", 100],
  ["trip-asks", 100],
  ["trip-asks", 50],
  ["trip-lead", 30],
  ["jobs", 30],
] as const;

test.each(LOOKS)("%s at %d%%", async (name, percent) => {
  await page.viewport(1440, 900);
  const base = name.replace(/-(live|lead|leadlive|asks)$/u, "");
  const found = await scene(base);
  if (!found) return;
  const asking = name.endsWith("-asks") ? askingTrip(found) : null;
  const loaded = asking
    ? asking.scene
    : name.endsWith("-live")
      ? midRun(found)
      : name.endsWith("-lead")
        ? leadTrip(found, false)
        : name.endsWith("-leadlive")
          ? leadTrip(found, true)
          : found;
  shown = base;
  const errors: string[] = [];
  const listen = (event: ErrorEvent) => errors.push(event.message);
  window.addEventListener("error", listen);
  const zoom = percent / 100;
  const screen = await render(BoardCanvas, {
    scene: loaded,
    ...(asking ? { asks: asking.asks } : {}),
    viewport: { x: zoom === 1 ? 120 : zoom < 0.4 ? 120 : 200, y: 72, zoom },
  });
  screen.container.style.width = "1440px";
  screen.container.style.height = "900px";
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await new Promise((done) => setTimeout(done, 900));
    await page.screenshot({ path: `${shots}/${name}-${percent}-${theme}.png` });
  }
  document.documentElement.dataset.theme = "dark";
  window.removeEventListener("error", listen);
  if (errors.length) console.error(name, errors.join(" | "));
  await screen.unmount();
});
