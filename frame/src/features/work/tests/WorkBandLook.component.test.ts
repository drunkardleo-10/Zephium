import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import BoardCanvas from "./BoardCanvas.svelte";
import type { BoardScene } from "./board-fixtures";
import type { MediaAssetV1 } from "$domain/resources";
import { elementPictures } from "../lib/project-environment-board";
import { askingTrip, leadTrip, needingPart } from "./lead-look";

// Real runs exported read-only from the QA profile into node_modules/.work-look;
// without them there is nothing to look at and the test only renders nothing.
const LOOK = "/node_modules/.work-look";
let shown = "";
/** Each admitted picture's file extension, by digest, for the scene on show. */
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
// The QA profile's cached site icons, so marks draw as they do in the app.
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

const shots = "../../../../../target/work-band";

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
  "yctrip",
  "compare",
  "today",
  "lunios",
  "trip3",
  "slack3",
  "compilers",
  "day",
];
/** 0 is the view a work opens in with no camera of its own. */
const LOOKS = [
  ...WORKS.flatMap(
    (name) =>
      [
        [name, 0],
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
  ["today-need", 100],
  ["lunios-need", 0],
] as const;

test.each(LOOKS)("%s at %d%%", async (name, percent) => {
  await page.viewport(1440, 900);
  const base = name.replace(/-(live|lead|leadlive|asks|need)$/u, "");
  const found = await scene(base);
  if (!found) return;
  const asking = name.endsWith("-asks") ? askingTrip(found) : null;
  const loaded = asking
    ? asking.scene
    : name === "today-need"
      ? needingPart(found, { kind: "sign_in", host: "app.slack.com" })
      : name === "lunios-need"
        ? needingPart(found, { kind: "allow_folder", path: "/Users/crynta/Dev/Lunios" })
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
    // A lead run draws what it drew on left of its request: the view makes room for it.
    ...(zoom
      ? {
          viewport: {
            x: (zoom === 1 ? 120 : zoom < 0.4 ? 120 : 200) + (base !== name ? 280 * zoom : 0),
            y: 72,
            zoom,
          },
        }
      : {}),
  });
  screen.container.style.width = "1440px";
  screen.container.style.height = "900px";
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await new Promise((done) => setTimeout(done, 900));
    await page.screenshot({
      path: `${shots}/${name}-${percent ? percent : "open"}-${theme}.png`,
    });
  }
  document.documentElement.dataset.theme = "dark";
  window.removeEventListener("error", listen);
  if (errors.length) console.error(name, errors.join(" | "));
  await screen.unmount();
});
