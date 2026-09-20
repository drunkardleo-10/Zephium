import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import ToolShelf from "../components/ToolShelf.svelte";

const native = vi.hoisted(() => ({
  run: vi.fn(async () => ({ accepted: true, operation_id: null })),
  toolsMenu: vi.fn(async () => true),
}));

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    runCommand: native.run,
    toolsMenuPopup: native.toolsMenu,
    sidebarSetWidth: vi.fn(async () => undefined),
  });
});

test("the mini-apps stay out of reach until the case is hovered", async () => {
  const screen = await render(ToolShelf);
  // The stack grows upward out of the case, so it needs room above it.
  screen.container.style.paddingBlockStart = "180px";
  const flyout = screen.container.querySelector<HTMLElement>(".flyout")!;
  const notes = screen.getByRole("button", { name: "Notes", exact: true });
  const tools = screen.getByRole("button", { name: "Tools", exact: true });

  // Inert, so the stack is neither clickable nor tabbable while concealed.
  expect(flyout.inert).toBe(true);

  await tools.hover();
  await expect.poll(() => flyout.inert).toBe(false);

  // Picking one puts the stack away; the panel it opened is the answer now.
  await notes.click();
  await expect.poll(() => flyout.inert).toBe(true);
  await expect.poll(() => screen.container.querySelector(".case-lit")).not.toBeNull();

  await tools.hover();
  await expect.poll(() => flyout.inert).toBe(false);
  await expect.element(notes).toHaveAttribute("aria-pressed", "true");
});

test("the case hands the whole list to the native menu", async () => {
  const screen = await render(ToolShelf);
  const tools = screen.getByRole("button", { name: "Tools", exact: true });

  await expect.element(tools).toHaveAttribute("aria-haspopup", "menu");
  await tools.click();
  expect(native.toolsMenu).toHaveBeenCalledOnce();
});
