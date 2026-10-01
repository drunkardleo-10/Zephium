import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import ObjectsSheet from "./ObjectsSheet.svelte";
import { centred, looks } from "./object-fixtures";
import type { DiagramView, ObjectView } from "../lib/board/types";

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

const shots = "../../../../../target/work-objects";
const settle = (ms = 700) => new Promise((done) => setTimeout(done, ms));

/**
 * Every object's renderer is in: a kind whose chunk has not loaded leaves its
 * cell empty. Waiting on a fixed delay shot the first group of a new kind blank.
 */
function loaded(sheet: HTMLElement): boolean {
  const cells = [...sheet.querySelectorAll<HTMLElement>(":scope > .row > :is(.cell, .well)")];
  return (
    cells.length > 0 && cells.every((cell) => !!cell.querySelector("*") && cell.offsetHeight >= 8)
  );
}

/** A row in view has drawn: its pictures decoded (or failed) and its charts in. */
function drawnRow(row: HTMLElement): boolean {
  if ([...row.querySelectorAll("img")].some((image) => !image.complete)) return false;
  return [...row.querySelectorAll("section.plot")].every(
    (plot) => !!plot.querySelector("figure.chart svg"),
  );
}

/**
 * The diagrams real runs drew, every one the QA profile holds (exported by
 * `export_diagrams.py` into node_modules/.work-look/diagrams), at the width a
 * result is given at 100%.
 */
async function drawn(): Promise<{ object: ObjectView; width: number }[]> {
  const response = await fetch("/node_modules/.work-look/diagrams/diagrams.json");
  if (!response.ok) return [];
  const saved = (await response.json()) as {
    id: string;
    title: string;
    diagram: DiagramView["diagram"];
  }[];
  return saved.map(({ id, title, diagram }) => ({
    object: { kind: "diagram", id, title, diagram },
    width: 1120,
  }));
}

const scenes: Record<string, () => Promise<{ object: ObjectView; width: number }[]>> = {
  ...Object.fromEntries(Object.entries(looks).map(([name, rows]) => [name, async () => rows])),
  diagrams: drawn,
  centre: async () => centred,
};

test.each(Object.keys(scenes))(
  "%s at 100% and at 50%",
  async (name) => {
    await page.viewport(2400, 1600);
    const noop = () => {};
    const rows = await scenes[name]!();
    if (!rows.length) return;
    const screen = await render(ObjectsSheet, {
      rows,
      centre: name === "centre",
      actions: {
        choose: noop,
        ask: noop,
        compare: noop,
        open: noop,
        link: noop,
        check: noop,
        send: noop,
        write: noop,
      },
    });
    const sheet = screen.container.querySelector<HTMLElement>(".sheet")!;
    await expect.poll(() => loaded(sheet), { timeout: 20000, interval: 100 }).toBe(true);
    for (const theme of ["dark", "light"]) {
      document.documentElement.dataset.theme = theme;
      await settle();
      for (const row of sheet.querySelectorAll<HTMLElement>(":scope > .row")) {
        // The shot is drawn at the scale the page is shown at: a viewport just
        // larger than the row keeps a small object at 2x and a whole diagram in view.
        const size = row.getBoundingClientRect();
        await page.viewport(
          Math.max(1152, Math.ceil(size.width) + 16),
          Math.max(720, Math.ceil(size.height) + 16),
        );
        row.scrollIntoView({ block: "start", inline: "start" });
        await expect.poll(() => drawnRow(row), { timeout: 20000, interval: 100 }).toBe(true);
        // Charts arrive once over the base motion; they are shot settled.
        await settle(theme === "dark" ? 400 : 120);
        await page
          .elementLocator(row)
          .screenshot({ path: `${shots}/${name}/${row.dataset.id}-${theme}.png` });
      }
    }
    document.documentElement.dataset.theme = "dark";
    await screen.unmount();
  },
  120_000,
);
