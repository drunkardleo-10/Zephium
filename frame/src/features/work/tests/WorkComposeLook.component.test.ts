import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import BoardCanvas from "./BoardCanvas.svelte";
import type { BoardScene } from "./board-fixtures";
import type { MediaAssetV1 } from "$domain/resources";
import { elementPictures } from "../lib/project-environment-board";

// Round-four runs exported read-only from the QA profile into node_modules/.work-look.
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

const shots = "../../../../../target/work-compose";

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

const WORKS = ["r4lego", "r4trip", "r4browsers", "r4code", "r4lunios"];
/** 0 is the view a work opens in; 100 and 50 stand at the canvas's top left, 50 on a large screen. */
const LOOKS: [string, number, number][] = [
  ...WORKS.flatMap((name): [string, number, number][] => [
    [name, 0, 0],
    [name, 100, 0],
    [name, 50, 0],
  ]),
  // The diagrams at full size, the view lowered onto them.
  ["r4code", 100, 400],
  ["r4browsers", 100, 440],
];

test.each(LOOKS)("%s at %d%% %d", async (name, percent, down) => {
  const wide = percent === 50 || down > 0;
  await page.viewport(wide ? 2400 : 1440, wide ? 1500 : 900);
  const found = await scene(name);
  if (!found) return;
  shown = name;
  const zoom = percent / 100;
  // The canvas's own saved things read as the person's notes, so a note is on show.
  const notes = found.snapshot.elements.flatMap((element) =>
    element.reference.kind === "resource" && !found.media?.has(element.reference.resource)
      ? [
          {
            id: element.reference.resource,
            revision: "1",
            title: "Questions for the review",
            preview:
              "Why does a timeout report OutcomeUnknown rather than a failure? Check the 8-slot limit under load.",
            pinned: false,
            trashed: false,
            editable: true,
            created_at: "1790700000000",
            modified_at: "1790719800000",
            path: "Questions for the review.md",
          },
        ]
      : [],
  );
  const screen = await render(BoardCanvas, {
    scene: found,
    notes,
    ...(zoom ? { viewport: { x: zoom === 1 ? 300 : 200, y: 72 - down, zoom } } : {}),
  });
  screen.container.style.width = wide ? "2400px" : "1440px";
  screen.container.style.height = wide ? "1500px" : "900px";
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await new Promise((done) => setTimeout(done, 900));
    await page.screenshot({
      path: `${shots}/${name}-${percent || "open"}${down ? "-low" : ""}-${theme}.png`,
    });
  }
  document.documentElement.dataset.theme = "dark";
  // What a node draws never runs past the room its run gave it, and no two nodes share room.
  const nodes = [...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].filter(
    (node) => !node.classList.contains("svelte-flow__node-agent"),
  );
  const zoomed = zoom || 1;
  const over = nodes.flatMap((node) => {
    const drawn = node.firstElementChild?.firstElementChild as HTMLElement | null | undefined;
    const room = node.offsetHeight;
    return drawn && drawn.scrollHeight > room + 2
      ? [`${node.dataset.id}: ${drawn.scrollHeight} > ${room}`]
      : [];
  });
  const boxes = nodes.map((node) => ({ id: node.dataset.id, box: node.getBoundingClientRect() }));
  const shared = boxes.flatMap((a, index) =>
    boxes.slice(index + 1).flatMap((b) => {
      const x = Math.min(a.box.right, b.box.right) - Math.max(a.box.left, b.box.left);
      const y = Math.min(a.box.bottom, b.box.bottom) - Math.max(a.box.top, b.box.top);
      return x > 2 * zoomed && y > 2 * zoomed ? [`${a.id} × ${b.id}`] : [];
    }),
  );
  await screen.unmount();
  expect(nodes.length).toBeGreaterThan(0);
  expect({ over, shared }).toEqual({ over: [], shared: [] });
});
