import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import WorkWorkspace from "../WorkWorkspace.svelte";
const native = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ resourceCall: native.call, workCall: native.call });
});
test("without a regular profile Work explains itself and issues no queries", async () => {
  const screen = await render(WorkWorkspace);
  screen.container.style.width = "900px";
  screen.container.style.height = "700px";
  await expect
    .element(screen.getByText("Choose a regular profile to use Work.", { exact: true }))
    .toBeVisible();
  expect(screen.container.querySelectorAll(".svelte-flow__node")).toHaveLength(0);
  expect(native.call).not.toHaveBeenCalled();
  await screen.unmount();
  expect(screen.container.querySelector(".svelte-flow")).toBeNull();
});
