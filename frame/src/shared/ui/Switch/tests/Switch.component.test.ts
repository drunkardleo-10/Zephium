import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { userEvent } from "vitest/browser";
import Switch from "../Switch.svelte";

test("switch has a stable label and keyboard behavior", async () => {
  const onchange = vi.fn();
  const screen = await render(Switch, {
    label: "Reduce motion",
    description: "Limit animation",
    onchange,
  });
  const control = screen.getByRole("switch", { name: "Reduce motion" });
  await expect.element(control).toHaveAttribute("aria-checked", "false");
  await expect.element(control).toHaveAccessibleDescription("Limit animation");
  await userEvent.keyboard(navigator.platform.includes("Mac") ? "{Alt>}{Tab}{/Alt}" : "{Tab}");
  await expect.element(control).toHaveFocus();
  await userEvent.keyboard(" ");
  expect(onchange).toHaveBeenCalledWith(true);
});
