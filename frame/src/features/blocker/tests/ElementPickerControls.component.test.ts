import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import type { BlockerPickerView, BlockerSiteContext } from "$shared/ipc/bindings";
import { commands } from "$shared/ipc/bindings";
import ElementPickerControls from "../components/ElementPickerControls.svelte";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ blockerPicker: vi.fn() });
});
const context: BlockerSiteContext = {
  profile: "profile",
  tab: "tab",
  site: "example.com",
  revision: "0000000000000001",
};
const ready: BlockerPickerView = { session: "0000000000000001", active: true, selection: null };
afterEach(() => vi.mocked(commands.blockerPicker).mockReset());

test("picker requires preview before save and cancellation names the exact session", async () => {
  const native = vi.mocked(commands.blockerPicker);
  native.mockResolvedValueOnce({ status: "ok", data: ready });
  const screen = await render(ElementPickerControls, { props: { context } });
  await screen.getByRole("menuitem", { name: "Hide an element…" }).click();
  await expect.element(screen.getByRole("menuitem", { name: "Preview selection" })).toBeVisible();
  expect(native).toHaveBeenLastCalledWith(context, { kind: "start" });
  expect(screen.container.textContent).not.toContain("Save hide");
  native.mockResolvedValueOnce({
    status: "ok",
    data: {
      ...ready,
      selection: { identity: "a".repeat(64), label: "Banner", count: 2, positional: true },
    },
  });
  await screen.getByRole("menuitem", { name: "Preview selection" }).click();
  await expect.element(screen.getByRole("menuitem", { name: "Save hide" })).toBeEnabled();
  expect(screen.container.textContent).toContain("2 matching elements");
  expect(screen.container.textContent).toContain("may change");
  native.mockResolvedValueOnce({ status: "ok", data: { ...ready, active: false } });
  await screen.getByRole("menuitem", { name: "Cancel", exact: true }).click();
  expect(native).toHaveBeenLastCalledWith(context, { kind: "stop", session: ready.session });
});

test("a picker that starts after its controls close is explicitly cancelled", async () => {
  let complete!: (value: { status: "ok"; data: BlockerPickerView }) => void;
  const native = vi.mocked(commands.blockerPicker);
  native.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        complete = resolve;
      }),
  );
  const screen = await render(ElementPickerControls, { props: { context } });
  await screen.getByRole("menuitem", { name: "Hide an element…" }).click();
  await screen.unmount();
  native.mockResolvedValueOnce({ status: "ok", data: { ...ready, active: false } });
  complete({ status: "ok", data: ready });
  await expect
    .poll(() => native.mock.calls.at(-1)?.[1])
    .toEqual({ kind: "stop", session: ready.session });
});
