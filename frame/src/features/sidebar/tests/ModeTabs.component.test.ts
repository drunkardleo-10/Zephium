import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { surface } from "$domain/surface";
import { emitNativeEvent } from "$shared/testing/native-events";
import ModeTabs from "../components/ModeTabs.svelte";

const native = vi.hoisted(() => ({
  run: vi.fn(async () => ({ accepted: true, operation_id: null })),
}));

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ runCommand: native.run });
});

afterEach(() => surface.dispose());

test("the lit half follows native confirmation rather than command admission", async () => {
  await surface.init();
  const screen = await render(ModeTabs);
  const browse = screen.getByRole("radio", { name: "Browse", exact: true });
  const work = screen.getByRole("radio", { name: "Work", exact: true });

  await expect.element(browse).toHaveAttribute("aria-checked", "true");

  await work.click();
  expect(native.run).toHaveBeenCalledWith("browser.work");
  // Admission is not arrival: the thumb stays on Browse until native says so.
  await expect.element(work).toHaveAttribute("aria-checked", "false");
  emitNativeEvent("uiCommand", "browser.work");
  await expect.element(work).toHaveAttribute("aria-checked", "true");
  await expect.element(browse).toHaveAttribute("aria-checked", "false");

  await browse.click();
  expect(native.run).toHaveBeenCalledWith("browser.return");
  emitNativeEvent("uiCommand", "browser.return");
  await expect.element(browse).toHaveAttribute("aria-checked", "true");
});

test("re-selecting the side already showing asks native for nothing", async () => {
  await surface.init();
  const screen = await render(ModeTabs);
  native.run.mockClear();

  await screen.getByRole("radio", { name: "Browse", exact: true }).click();
  expect(native.run).not.toHaveBeenCalled();
});

test("the rail carries the same switch as glyphs, each still named", async () => {
  await surface.init();
  const screen = await render(ModeTabs, { compact: true });
  const work = screen.getByRole("radio", { name: "Work", exact: true });
  await expect.element(work).toHaveAttribute("title", "Work");
  expect(screen.container.querySelector(".segmented")?.getAttribute("data-icon-only")).toBe("true");

  await work.click();
  expect(native.run).toHaveBeenCalledWith("browser.work");
  emitNativeEvent("uiCommand", "browser.work");
  await expect.element(work).toHaveAttribute("aria-checked", "true");
});
