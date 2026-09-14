import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import WorkWorkspace from "../WorkWorkspace.svelte";
const mocks = vi.hoisted(() => ({
  openUrl: vi.fn(),
  settle: vi.fn(),
  openSurface: vi.fn(),
  flush: vi.fn(),
}));
vi.mock("$domain/tabs", () => ({
  tabs: {
    profile: () => ({ id: "profile", kind: "regular", name: "Reader" }),
    activeSpaceId: () => "space",
    tabs: () => [],
    spaces: () => [{ id: "space", name: "Space" }],
  },
}));
vi.mock("$domain/work-environment", () => ({
  environmentSession: () => ({
    profile: "profile",
    space: "space",
    start: async () => {},
    stopObserving: () => {},
    flushView: mocks.flush,
  }),
}));
vi.mock("$domain/preferences", () => ({ preferences: { value: () => "true" } }));
vi.mock("$domain/operations", () => ({ settle: mocks.settle }));
vi.mock("$domain/surface", () => ({
  surface: { open: mocks.openSurface, navigationFailed: () => false },
}));
vi.mock("$features/work", () => ({
  loadWorkEnvironmentWorkspace: () => import("./WorkSourceHarness.svelte"),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ tabsOpenUrl: mocks.openUrl });
});
test.each(["applied", "no_op", "deferred", "rejected", "failed"] as const)(
  "source navigation handles %s without inventing success",
  async (outcome) => {
    vi.clearAllMocks();
    mocks.flush.mockResolvedValue(true);
    mocks.openUrl.mockResolvedValue({ accepted: true, operation_id: "0000000000000001" });
    mocks.settle.mockResolvedValue({ outcome });
    const screen = await render(WorkWorkspace);
    await screen.getByRole("button", { name: "Open source in Browse", exact: true }).click();
    await expect.poll(() => mocks.settle.mock.calls.length).toBe(1);
    expect(mocks.openUrl).toHaveBeenCalledExactlyOnceWith("https://example.com/source");
    if (outcome === "applied" || outcome === "no_op")
      expect(mocks.openSurface).toHaveBeenCalledExactlyOnceWith(null);
    else expect(mocks.openSurface).not.toHaveBeenCalled();
    if (outcome === "failed" || outcome === "rejected")
      await expect.element(screen.getByRole("status")).toBeVisible();
    else expect(screen.container.querySelector(".navigation-error")).toBeNull();
    await screen.unmount();
  },
);
test("an unflushed Work does not open a source", async () => {
  vi.clearAllMocks();
  mocks.flush.mockResolvedValue(false);
  const screen = await render(WorkWorkspace);
  await screen.getByRole("button", { name: "Open source in Browse", exact: true }).click();
  expect(mocks.flush).toHaveBeenCalledOnce();
  expect(mocks.openUrl).not.toHaveBeenCalled();
  expect(mocks.openSurface).not.toHaveBeenCalled();
  await screen.unmount();
});
