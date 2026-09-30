import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import BoardCanvas from "./BoardCanvas.svelte";
import type { BoardScene } from "./board-fixtures";
import type { MediaAssetV1 } from "$domain/resources";
import { elementPictures } from "../lib/project-environment-board";

// Today's QA runs exported read-only into node_modules/.work-look; without them nothing renders.
const LOOK = "/node_modules/.work-look";
let shown = "";
const pictureFiles = new Map<string, string>();
const EXTENSION: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/webp": "webp",
  "image/gif": "gif",
};
vi.mock("$domain/resources", async (original) => ({
  ...(await original<typeof import("$domain/resources")>()),
  pageFrameUrl: (attempt: string, step: string) => `${LOOK}/${shown}/frames/${attempt}-${step}.png`,
  mediaUrl: (_profile: string, digest: string) =>
    `${LOOK}/${shown}/media/${digest}.${pictureFiles.get(digest) ?? "png"}`,
}));
vi.mock("$domain/favicons", async (original) => {
  const actual = await original<typeof import("$domain/favicons")>();
  const response = await fetch("/node_modules/.work-look/objects/favicons.json");
  const icons = response.ok ? ((await response.json()) as Record<string, string>) : {};
  const images = new Map<string, ImageData>();
  for (const [origin, rgba] of Object.entries(icons)) {
    const bytes = Uint8ClampedArray.from(atob(rgba), (char) => char.charCodeAt(0));
    if (bytes.length === 4096) images.set(origin, new ImageData(bytes, 32, 32));
  }
  const forPage = (url: string) => {
    const origin = /^https?:\/\/[^/?#]+/iu.exec(url)?.[0]?.toLowerCase();
    const image = origin ? images.get(origin) : undefined;
    return image ? { image, tone: "mid" as const } : null;
  };
  return { ...actual, favicons: { ...actual.favicons, forPage } };
});
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ faviconProbe: async () => true });
});

const shots = "../../../../../target/work-release";

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
  pictureFiles.clear();
  for (const asset of media.values())
    pictureFiles.set(asset.digest, EXTENSION[asset.mime] ?? "png");
  return {
    name,
    snapshot: raw.snapshot,
    objectives: new Map(Object.entries(raw.objectives)),
    pictures: elementPictures(raw.snapshot, media),
    pages: raw.pages,
    media,
  };
}

/**
 * The first run caught at its `at`-th read: parts with steps still ahead are
 * running, the step itself is live, and only what was made so far is shown.
 */
function midRun(loaded: BoardScene, at: number, only = 1): BoardScene {
  const objectives = new Map(
    [...loaded.objectives].map(([id, projection]) => {
      const copy = structuredClone(projection);
      copy.executions = copy.executions.slice(0, only);
      const execution = copy.executions.at(-1);
      const steps = execution?.steps ?? [];
      const reads = steps.flatMap((step, index) => (step.kind.kind === "read" ? [index] : []));
      const cut = reads[Math.min(at, reads.length - 1)];
      if (execution && cut !== undefined) {
        const ahead = new Set(steps.slice(cut).flatMap((step) => (step.part ? [step.part] : [])));
        execution.status = "running";
        execution.steps = steps.slice(0, cut + 1);
        execution.steps[cut] = { ...execution.steps[cut]!, status: "running" };
        const lastSearch = execution.steps.findLastIndex((step) => step.kind.kind === "search");
        if (lastSearch >= 0 && execution.steps[lastSearch]!.part !== execution.steps[cut]!.part)
          execution.steps[lastSearch] = { ...execution.steps[lastSearch]!, status: "running" };
        execution.parts = (execution.parts ?? []).map((part) =>
          ahead.has(part.id) || !part.ended_ms
            ? { ...part, state: "running", summary: null, ended_ms: null }
            : part,
        );
        execution.artifacts = execution.artifacts.filter((artifact) =>
          execution.steps!.some((step) => step.artifacts?.includes(artifact.id)),
        );
      }
      return [id, copy] as const;
    }),
  );
  return { ...loaded, objectives };
}

const only = (import.meta.env.VITE_LOOK as string | undefined)?.split(",");
const LOOKS: [string, number][] = [
  ["research", 0],
  ["research", 50],
  ["blueprint", 50],
  ["research-live", 100],
  ["blueprint", 0],
  ["trip5", 0],
  ["trip5-live", 70],
  ["lego5", 0],
  ["lego5", 50],
  ["wednesday", 0],
  ["cando", 0],
];

test.each(
  LOOKS.filter(
    ([name, percent]) => !only || only.some((one) => one === name || one === `${name}@${percent}`),
  ),
)("%s at %d%%", async (name, percent) => {
  await page.viewport(1440, 900);
  const base = name.replace(/-live$/u, "");
  const found = await scene(base);
  if (!found) return;
  const loaded = name.endsWith("-live") ? midRun(found, base === "research" ? 3 : 2) : found;
  shown = base;
  const errors: string[] = [];
  const listen = (event: ErrorEvent) => errors.push(event.message);
  window.addEventListener("error", listen);
  const zoom = percent / 100;
  const screen = await render(BoardCanvas, {
    scene: loaded,
    ...(zoom ? { viewport: { x: 120, y: 72, zoom } } : {}),
  });
  screen.container.style.width = "1440px";
  screen.container.style.height = "900px";
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await new Promise((done) => setTimeout(done, 900));
    await page.screenshot({ path: `${shots}/${name}-${percent || "open"}-${theme}.png` });
  }
  document.documentElement.dataset.theme = "dark";
  window.removeEventListener("error", listen);
  if (errors.length) console.error(name, errors.join(" | "));
  await screen.unmount();
});
