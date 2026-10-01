import "$styles/global.css";
import { test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import PresenceSheet from "./PresenceSheet.svelte";

const shots = "../../../../../../target/work-presence";

test("the characters and the working indicators, in both themes", async () => {
  await page.viewport(1400, 1800);
  const screen = await render(PresenceSheet);
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await new Promise((done) => setTimeout(done, 700));
    await page.screenshot({ path: `${shots}/sheet-${theme}.png` });
  }
  document.documentElement.dataset.theme = "dark";
  await screen.unmount();
});
