import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { userEvent } from "vitest/browser";
import Checkbox from "../Checkbox.svelte";

test("mixed checkbox exposes its description and supports keyboard activation", async () => {
  const onchange = vi.fn();
  const screen = await render(Checkbox, {
    label: "Select tabs",
    description: "Some tabs are selected",
    indeterminate: true,
    onchange,
  });
  const checkbox = screen.getByRole("checkbox", { name: "Select tabs" });
  await expect.element(checkbox).toHaveAttribute("aria-checked", "mixed");
  await expect.element(checkbox).toHaveAccessibleDescription("Some tabs are selected");
  await userEvent.keyboard(navigator.platform.includes("Mac") ? "{Alt>}{Tab}{/Alt}" : "{Tab}");
  await expect.element(checkbox).toHaveFocus();
  await userEvent.keyboard(" ");
  expect(onchange).toHaveBeenCalledWith(true);
});

test("disabled checkbox cannot receive tab focus or activate", async () => {
  const onchange = vi.fn();
  const screen = await render(Checkbox, { label: "Select tabs", disabled: true, onchange });
  const checkbox = screen.getByRole("checkbox", { name: "Select tabs" });
  await expect.element(checkbox).toBeDisabled();
  await userEvent.keyboard(navigator.platform.includes("Mac") ? "{Alt>}{Tab}{/Alt}" : "{Tab}");
  await expect.element(checkbox).not.toHaveFocus();
  await userEvent.keyboard(" ");
  expect(onchange).not.toHaveBeenCalled();
});
