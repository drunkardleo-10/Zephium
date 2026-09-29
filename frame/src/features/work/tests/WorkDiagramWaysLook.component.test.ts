import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import DiagramWays from "./DiagramWays.svelte";
import { layoutDiagram } from "../lib/diagram";
import type { DiagramView } from "../lib/board/types";

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

const shots = "../../../../../target/work-diagrams";
const settle = (ms = 400) => new Promise((done) => setTimeout(done, ms));

/** The real diagrams the QA profile holds (`export_diagrams.py`), each drawn both ways. */
async function real(): Promise<DiagramView[]> {
  const response = await fetch("/node_modules/.work-look/diagrams/diagrams.json");
  if (!response.ok) return [];
  const saved = (await response.json()) as {
    id: string;
    title: string;
    diagram: DiagramView["diagram"];
  }[];
  return saved.map(({ id, title, diagram }) => ({ kind: "diagram", id, title, diagram }));
}

test.each(["down", "right"] as const)(
  "real diagrams read %s, at 100% and 50%, both themes",
  async (way) => {
    const diagrams = await real();
    if (!diagrams.length) return;
    // Settled before they are drawn, so every shot is the engine's layout.
    for (const diagram of diagrams) await layoutDiagram(diagram.diagram, way);
    for (const zoom of [1, 0.5]) {
      await page.viewport(1400, 1000);
      const screen = await render(DiagramWays, { diagrams, way, zoom });
      for (const theme of ["dark", "light"]) {
        document.documentElement.dataset.theme = theme;
        await settle();
        for (const row of screen.container.querySelectorAll<HTMLElement>(".row")) {
          const size = row.getBoundingClientRect();
          await page.viewport(
            Math.max(800, Math.ceil(size.width) + 16),
            Math.max(600, Math.ceil(size.height) + 16),
          );
          row.scrollIntoView({ block: "start", inline: "start" });
          await expect
            .poll(() => [...row.querySelectorAll("img")].every((image) => image.complete))
            .toBe(true);
          await settle(120);
          const tag = zoom === 1 ? "" : "-50";
          await page
            .elementLocator(row)
            .screenshot({ path: `${shots}/${row.dataset.id}-${way}${tag}-${theme}.png` });
        }
      }
      document.documentElement.dataset.theme = "dark";
      await screen.unmount();
    }
  },
  240_000,
);
