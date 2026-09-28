import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import ObjectsSheet from "./ObjectsSheet.svelte";
import { looks } from "./object-fixtures";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ faviconProbe: async () => true });
});

const shots = "../../../../../target/work-objects";
const settle = (ms = 700) => new Promise((done) => setTimeout(done, ms));

test.each(Object.keys(looks))("%s at full, overview and tile", async (name) => {
  await page.viewport(1440, 900);
  const screen = await render(ObjectsSheet, { rows: looks[name]! });
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
