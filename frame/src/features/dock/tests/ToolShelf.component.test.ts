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

test("the tools stay out of reach until the case is pressed", async () => {
  const screen = await render(ToolShelf);
  // The stack grows upward out of the case, so it needs room above it.
  screen.container.style.paddingBlockStart = "280px";
  const stack = screen.container.querySelector<HTMLElement>(".shelf-stack")!;
  const tools = screen.getByRole("button", { name: "Tools", exact: true });

  // Inert, so the stack is neither clickable nor tabbable while concealed,
  // and hovering the case is not a request for it.
  expect(stack.inert).toBe(true);
  await tools.hover();
  expect(stack.inert).toBe(true);

  await tools.click();
  await expect.poll(() => stack.inert).toBe(false);
  await expect.element(tools).toHaveAttribute("aria-expanded", "true");

  // Picking one puts the stack away; the panel it opened is the answer now.
  await screen.getByRole("menuitem", { name: "Notes", exact: true }).click();
  await expect.poll(() => stack.inert).toBe(true);
  await expect.poll(() => screen.container.querySelector(".case-lit")).not.toBeNull();

  await tools.click();
  await expect.poll(() => stack.inert).toBe(false);
  await expect
    .element(screen.getByRole("menuitem", { name: "Notes", exact: true }))
    .toHaveAttribute("aria-current", "true");
});

test("the records follow the tools, and the whole list is a secondary click away", async () => {
  const screen = await render(ToolShelf);
  const tools = screen.getByRole("button", { name: "Tools", exact: true });
  await expect.element(tools).toHaveAttribute("aria-haspopup", "menu");

  const names = [...screen.container.querySelectorAll(".shelf-item")].map((item) =>
    item.textContent?.trim(),
  );
  expect(names).toEqual(["Notes", "Tasks", "Activity", "Ask", "History", "Downloads"]);

  await tools.click({ button: "right" });
  expect(native.toolsMenu).toHaveBeenCalledOnce();
});
