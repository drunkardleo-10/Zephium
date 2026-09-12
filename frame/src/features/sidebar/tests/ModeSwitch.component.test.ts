import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { surface } from "$domain/surface";
import { emitNativeEvent } from "$shared/testing/native-events";
import ModeSwitch from "../components/ModeSwitch.svelte";
const native = vi.hoisted(() => ({
  run: vi.fn(async () => ({ accepted: true, operation_id: null })),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ runCommand: native.run });
});
afterEach(() => surface.dispose());
test("mode selection follows native confirmation rather than command admission", async () => {
  await surface.init();
  const screen = await render(ModeSwitch);
  const work = screen.getByRole("button", { name: "Work", exact: true });
  await work.click();
  expect(native.run).toHaveBeenCalledWith("browser.work");
  await expect.element(work).toHaveAttribute("aria-pressed", "false");
  emitNativeEvent("uiCommand", "browser.work");
  await expect.element(work).toHaveAttribute("aria-pressed", "true");
  const browse = screen.getByRole("button", { name: "Browse", exact: true });
  await browse.click();
  expect(native.run).toHaveBeenCalledWith("browser.return");
  await expect.element(work).toHaveAttribute("aria-pressed", "true");
  emitNativeEvent("uiCommand", "browser.return");
  await expect.element(browse).toHaveAttribute("aria-pressed", "true");
});
