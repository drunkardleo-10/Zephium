import "$styles/global.css";
import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import BarMenuStage from "./BarMenuStage.svelte";

test("the composer stays open while a menu it opened is in use, in its portal too", async () => {
  await page.viewport(1000, 600);
  const screen = await render(BarMenuStage);
  const bar = () => screen.container.querySelector<HTMLElement>(".work-bar")!;
  await screen.getByRole("textbox").click();
  await expect.poll(() => bar().dataset.engaged).toBe("true");
  await screen.getByRole("button", { name: "Model" }).click();
  await expect.element(page.getByRole("button", { name: "Opus" })).toBeVisible();
  await expect.poll(() => bar().dataset.engaged).toBe("true");
  await page.getByRole("button", { name: "Opus" }).click();
  await expect.poll(() => bar().dataset.engaged).toBe("true");
  // Leaving the bar for the canvas lets it go.
  await userEvent.click(document.body, { position: { x: 20, y: 20 } });
  await expect.poll(() => bar().dataset.engaged).toBe("false");
  await screen.unmount();
});
