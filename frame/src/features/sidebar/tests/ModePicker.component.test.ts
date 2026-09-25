import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { surface } from "$domain/surface";
import { emitNativeEvent } from "$shared/testing/native-events";
import ModePicker from "../components/ModePicker.svelte";

const native = vi.hoisted(() => ({
  run: vi.fn(async () => ({ accepted: true, operation_id: null })),
}));

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ runCommand: native.run });
});

afterEach(() => surface.dispose());

test("the rail names the environment and hands the choice to the native menu", async () => {
  await surface.init();
  const screen = await render(ModePicker);
  const trigger = screen.getByRole("button", { name: "Switch between Browse and Work" });

  await expect.element(trigger).toHaveTextContent("Browse");
  await expect.element(trigger).toHaveAttribute("aria-haspopup", "menu");

  await trigger.click();
  expect(native.run).toHaveBeenCalledWith("mode.choose");

  // The name is the confirmed environment, never the requested one.
  emitNativeEvent("uiCommand", "browser.work");
  await expect.element(trigger).toHaveTextContent("Work");
});
