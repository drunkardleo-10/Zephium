import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import ObjectsSheet from "./ObjectsSheet.svelte";
import { looks } from "./object-fixtures";
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

/** The diagrams real runs drew, from the QA export. */
async function drawn(): Promise<{ object: ObjectView; width: number }[]> {
  const out: { object: ObjectView; width: number }[] = [];
  for (const name of ["aisaas", "browser", "saas"]) {
    const response = await fetch(`/node_modules/.work-look/${name}/scene.json`);
    if (!response.ok) continue;
    const scene = (await response.json()) as {
      objectives: Record<
        string,
        {
          executions: {
            artifacts: {
              id: string;
              title: string;
              data: { kind: string } & Record<string, unknown>;
            }[];
          }[];
        }
      >;
    };
    for (const objective of Object.values(scene.objectives))
      for (const execution of objective.executions)
        for (const artifact of execution.artifacts) {
          if (artifact.data.kind !== "diagram") continue;
          const data = artifact.data as unknown as DiagramView["diagram"];
          out.push({
            object: {
              kind: "diagram",
              id: artifact.id,
              title: artifact.title,
              diagram: {
                kind: "diagram",
                nodes: data.nodes,
                edges: data.edges,
                layers: data.layers ?? [],
              },
            },
            width: 1500,
          });
        }
  }
  return out;
}

const scenes: Record<string, () => Promise<{ object: ObjectView; width: number }[]>> = {
  ...Object.fromEntries(Object.entries(looks).map(([name, rows]) => [name, async () => rows])),
  diagrams: drawn,
};

test.each(Object.keys(scenes))("%s at full, overview and tile", async (name) => {
  await page.viewport(2400, 1600);
  const noop = () => {};
  const rows = await scenes[name]!();
  if (!rows.length) return;
  const screen = await render(ObjectsSheet, {
    rows,
    actions: {
      choose: noop,
      ask: noop,
      compare: noop,
      open: noop,
      link: noop,
      check: noop,
      send: noop,
    },
  });
  const sheet = screen.container.querySelector<HTMLElement>(".sheet")!;
  await settle(1800);
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await settle();
    for (const row of sheet.querySelectorAll<HTMLElement>(".row")) {
      row.scrollIntoView({ block: "start" });
      await settle(120);
      await page
        .elementLocator(row)
        .screenshot({ path: `${shots}/${name}/${row.dataset.id}-${theme}.png` });
    }
  }
  document.documentElement.dataset.theme = "dark";
  await screen.unmount();
});
