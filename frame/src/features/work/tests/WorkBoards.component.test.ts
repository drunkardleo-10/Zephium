import "$styles/global.css";
import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import { favicons } from "$domain/favicons";
import { emitNativeEvent } from "$shared/testing/native-events";
import BoardCanvas from "./BoardCanvas.svelte";
import {
  dinnerScene,
  jobsScene,
  runningScene,
  rustScene,
  saasScene,
  tripScene,
  type BoardScene,
} from "./board-fixtures";
import { layoutDiagram } from "../lib/diagram";

/** Admitted pictures and page frames are served by native; here they are drawn. */
vi.mock("$domain/resources", async (original) => {
  const actual = await original<typeof import("$domain/resources")>();
  const svg = (body: string, width = 640, height = 400) =>
    `data:image/svg+xml,${encodeURIComponent(
      `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">${body}</svg>`,
    )}`;
  const photo = (hue: number) =>
    svg(
      `<defs><linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="hsl(${hue} 45% 78%)"/><stop offset="1" stop-color="hsl(${hue + 20} 35% 55%)"/></linearGradient></defs>` +
        `<rect width="640" height="400" fill="url(#s)"/>` +
        `<rect x="80" y="150" width="300" height="190" rx="6" fill="hsl(${hue + 180} 18% 92%)"/>` +
        `<rect x="110" y="190" width="70" height="60" fill="hsl(${hue + 200} 30% 45%)"/><rect x="220" y="190" width="120" height="60" fill="hsl(${hue + 200} 30% 45%)"/>` +
        `<path d="M40 150 L230 60 L420 150 Z" fill="hsl(${hue + 10} 30% 38%)"/>` +
        `<rect y="330" width="640" height="70" fill="hsl(${hue + 90} 30% 40%)"/>` +
        `<circle cx="520" cy="90" r="36" fill="hsl(45 90% 85%)"/>`,
    );
  const frame = (seed: string) =>
    svg(
      `<rect width="1280" height="800" fill="#fbfbfa"/><rect width="1280" height="64" fill="#1d1d1f"/>` +
        `<rect x="48" y="112" width="560" height="36" rx="4" fill="#222"/>` +
        Array.from(
          { length: 9 },
          (_, index) =>
            `<rect x="48" y="${188 + index * 34}" width="${420 + ((index * 97 + seed.length * 13) % 480)}" height="14" rx="3" fill="#c9c9cc"/>`,
        ).join("") +
        `<rect x="820" y="112" width="400" height="300" rx="10" fill="hsl(${(seed.length * 47) % 360} 40% 70%)"/>`,
      1280,
      800,
    );
  return {
    ...actual,
    mediaUrl: (_profile: string, digest: string) => photo(parseInt(digest.slice(0, 3), 16) % 360),
    pageFrameUrl: (_attempt: string, step: string) => frame(step),
  };
});

afterEach(() => {
  favicons.dispose();
  delete document.documentElement.dataset.theme;
});

const SHOTS = "../../../../../target/boards";

/** A site's mark as native would deliver it: a 32 px raster, here a lettered tile in the site's colour. */
function mark(host: string): string {
  const canvas = new OffscreenCanvas(32, 32);
  const context = canvas.getContext("2d")!;
  let hue = 0;
  for (const char of host) hue = (hue * 31 + char.charCodeAt(0)) % 360;
  context.fillStyle = `hsl(${hue} 62% 48%)`;
  context.beginPath();
  context.roundRect(0, 0, 32, 32, 7);
  context.fill();
  context.fillStyle = "#fff";
  context.font = "600 19px -apple-system, sans-serif";
  context.textAlign = "center";
  context.textBaseline = "middle";
  context.fillText(host.replace(/^www\./u, "")[0]!.toUpperCase(), 16, 17);
  const bytes = context.getImageData(0, 0, 32, 32).data;
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}
const SITES = [
  "www.ycombinator.com",
  "esta.cbp.dhs.gov",
  "www.airbnb.com",
  "www.lot.com",
  "www.google.com",
  "www.kayak.com",
  "linear.app",
  "linear.com",
  "vercel.com",
  "figma.com",
  "raycast.com",
  "supabase.com",
  "arc.com",
  "notion.com",
  "stripe.com",
  "zunicafe.com",
  "nopasf.com",
  "kinkhao.com",
  "cloudflare.com",
  "postgresql.org",
  "redis.io",
  "aws.amazon.com",
  "clerk.com",
];
async function marks() {
  await favicons.init();
  emitNativeEvent("favicons", {
    surface: "chrome",
    profile_id: "profile",
    entries: SITES.map((host) => ({ origin: `https://${host}`, revision: "a", rgba: mark(host) })),
  });
}

/** Waits until nothing on the canvas has moved for a while: measured, laid out, arrived. */
async function settle(container: HTMLElement) {
  let last = "";
  let still = 0;
  for (let round = 0; round < 60 && still < 3; round += 1) {
    await new Promise((resolve) => setTimeout(resolve, 150));
    const now = [...container.querySelectorAll<HTMLElement>(".svelte-flow__node")]
      .map(
        (node) =>
          `${node.dataset.id}:${node.style.transform}:${node.style.width}:${node.style.height}`,
      )
      .join("|");
    still = now === last ? still + 1 : 0;
    last = now;
  }
  await new Promise((resolve) => setTimeout(resolve, 400));
}

/** Every card of a lane stands clear of every other: no two nodes overlap. */
function overlaps(container: HTMLElement): string[] {
  const nodes = [
    ...container.querySelectorAll<HTMLElement>(".svelte-flow__node:not(.svelte-flow__node-agent)"),
  ].map((node) => ({ id: node.dataset.id ?? "", rect: node.getBoundingClientRect() }));
  const out: string[] = [];
  for (const [index, a] of nodes.entries())
    for (const b of nodes.slice(index + 1))
      if (
        a.rect.left < b.rect.right - 1 &&
        b.rect.left < a.rect.right - 1 &&
        a.rect.top < b.rect.bottom - 1 &&
        b.rect.top < a.rect.bottom - 1
      )
        out.push(`${a.id} × ${b.id}`);
  return out;
}

async function board(
  name: string,
  make: () => BoardScene,
  size: { width: number; height: number },
) {
  const scene = make();
  for (const execution of [...scene.objectives.values()][0]!.executions)
    for (const artifact of execution.artifacts)
      if (artifact.data.kind === "diagram")
        await layoutDiagram({
          kind: "diagram",
          nodes: artifact.data.nodes.map((node) => ({
            ...node,
            vendor: node.vendor ?? undefined,
            note: node.note ?? undefined,
            layer: node.layer ?? undefined,
          })),
          edges: artifact.data.edges.map((edge) => ({ ...edge, label: edge.label ?? undefined })),
          layers: artifact.data.layers ?? [],
        } as Parameters<typeof layoutDiagram>[0]);
  await page.viewport(size.width, size.height);
  await marks();
  for (const theme of ["dark", "light"] as const) {
    document.documentElement.dataset.theme = theme;
    const screen = await render(BoardCanvas, { scene });
    screen.container.style.width = `${size.width}px`;
    screen.container.style.height = `${size.height}px`;
    await expect.poll(() => screen.container.querySelectorAll(".block").length).toBeGreaterThan(0);
    await settle(screen.container);
    expect(overlaps(screen.container)).toEqual([]);
    await page.screenshot({ path: `${SHOTS}/${name}-${theme}.png` });
    await screen.unmount();
    screen.container.remove();
  }
}

test("the SaaS architecture reads as a board: the diagram leads, the prose and tables follow", async () => {
  await board("saas", saasScene, { width: 2040, height: 2200 });
});

test("the YC trip: stays and flights as galleries with pictures, the checklist, the sources", async () => {
  await board("trip", tripScene, { width: 1720, height: 1500 });
});

test("the job search: eight roles from six sites", async () => {
  await board("jobs", jobsScene, { width: 1720, height: 1300 });
});

test("a Rust explanation pairs the prose with its example", async () => {
  await board("rust", rustScene, { width: 1720, height: 1100 });
});

test("dinner: three places with pictures, what each page said on each", async () => {
  await board("dinner", dinnerScene, { width: 1600, height: 1000 });
});

test("a live run: the column shows its pages, the board waits for the answer", async () => {
  await board("running", runningScene, { width: 1400, height: 900 });
});

test("a brief opens in place: it takes the board's width and its neighbours make room", async () => {
  await page.viewport(1720, 2300);
  const scene = saasScene();
  const screen = await render(BoardCanvas, { scene });
  screen.container.style.width = "1720px";
  screen.container.style.height = "2300px";
  await settle(screen.container);
  const node = (title: string) =>
    [...screen.container.querySelectorAll<HTMLElement>(".svelte-flow__node")].find(
      (candidate) => candidate.querySelector("h3")?.textContent === title,
    )!;
  const stack = () => node("Recommended stack").getBoundingClientRect();
  const checklist = () => node("Launch checklist").getBoundingClientRect();
  const before = { stack: stack(), checklist: checklist() };
  expect(screen.container.textContent).not.toContain("Show less");
  (screen.container.querySelector(".document footer .more") as HTMLElement).click();
  await settle(screen.container);
  const brief = node("Architecture brief").getBoundingClientRect();
  const board = node("Reference architecture").getBoundingClientRect();
  expect(Math.round(brief.width)).toBe(Math.round(board.width));
  // It leaves the checklist's row for one of its own under it; what stood above stays.
  expect(brief.top).toBeGreaterThan(checklist().bottom);
  expect(stack().top).toBe(before.stack.top);
  expect(overlaps(screen.container)).toEqual([]);
  await expect.element(screen.getByText("Show less", { exact: true })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/saas-open-dark.png` });
  (screen.getByText("Show less", { exact: true }).element() as HTMLElement).click();
  await settle(screen.container);
  expect(node("Architecture brief").getBoundingClientRect().top).toBe(before.checklist.top);
  await screen.unmount();
  screen.container.remove();
});

test("a diagram part lights its flows and opens a popover beside itself, never the diagram again", async () => {
  await page.viewport(1720, 1200);
  const scene = saasScene();
  const asked: string[] = [];
  const screen = await render(BoardCanvas, { scene, asked });
  screen.container.style.width = "1720px";
  screen.container.style.height = "1200px";
  await settle(screen.container);
  const part = screen.container.querySelector<HTMLElement>('[data-part="api"]')!;
  part.dispatchEvent(new PointerEvent("pointerenter"));
  await expect.poll(() => screen.container.querySelectorAll(".flow.lit").length).toBeGreaterThan(3);
  expect(screen.container.querySelectorAll(".part.dim").length).toBeGreaterThan(0);
  part.click();
  const popover = screen.getByRole("dialog", { name: "Core API" });
  await expect.element(popover).toBeVisible();
  const box = popover.element().getBoundingClientRect();
  const anchor = part.getBoundingClientRect();
  expect(box.left).toBeGreaterThanOrEqual(anchor.right);
  expect(Math.abs(box.top - anchor.top)).toBeLessThan(4);
  expect(popover.element().textContent).toContain("Postgres");
  await page.screenshot({ path: `${SHOTS}/saas-part-dark.png` });
  await screen.getByRole("button", { name: "Ask about this" }).click();
  expect(asked).toEqual(["Core API"]);
  await expect.element(popover).not.toBeInTheDocument();
  await screen.unmount();
  screen.container.remove();
});
