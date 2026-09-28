import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import PartsCanvas from "./PartsCanvas.svelte";
import { fixScene } from "./parts-look";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ faviconProbe: async () => true });
});

const shots = "../../../../../target/work-parts";

const cases = [
  { stage: "done", view: "full", zoom: 1 },
  { stage: "working", view: "full", zoom: 1 },
  { stage: "handoff", view: "full", zoom: 1 },
  { stage: "done", view: "overview", zoom: 0.5 },
  { stage: "working", view: "overview", zoom: 0.5 },
  { stage: "done", view: "tile", zoom: 0.3 },
] as const;

for (const { stage, view, zoom } of cases)
  test(`Fix-the-bug run with GitHub and Code parts, ${stage} at ${view}`, async () => {
    await page.viewport(1500, 760);
    const screen = await render(PartsCanvas, {
      scene: fixScene(stage),
      viewport: { x: 30, y: 30, zoom },
    });
    screen.container.style.width = "1500px";
    screen.container.style.height = "760px";
    for (const theme of ["dark", "light"]) {
      document.documentElement.dataset.theme = theme;
      await new Promise((done) => setTimeout(done, 800));
      await page.screenshot({ path: `${shots}/fix-${stage}-${view}-${theme}.png` });
    }
    document.documentElement.dataset.theme = "dark";
    await screen.unmount();
  });
