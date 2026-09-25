import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import Harness from "./Harness.svelte";
test("contains render errors and can reconstruct the view without exposing the exception", async () => {
  const screen = await render(Harness, { fail: true });
  await expect.element(screen.getByRole("alert")).toHaveTextContent("Cannot display this view");
  expect(screen.container.textContent).not.toContain("private render details");
  await screen.rerender({ fail: false });
  await screen.getByRole("button", { name: "Retry" }).click();
  await expect.element(screen.getByText("Restored view", { exact: true })).toBeVisible();
});
