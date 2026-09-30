import "$styles/global.css";
import { test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import ChartSheet from "./ChartSheet.svelte";

const shots = "../../../../../../../target/work-chart";

test("the charts, in both themes", async () => {
  await page.viewport(1240, 1700);
  const screen = await render(ChartSheet);
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await new Promise((done) => setTimeout(done, 900));
    await page.screenshot({ path: `${shots}/sheet-${theme}.png` });
  }
  document.documentElement.dataset.theme = "dark";
  await screen.unmount();
});
