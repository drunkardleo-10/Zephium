import { expect, test, vi } from "vitest";
import type { WorkEnvironmentSnapshot, WorkResponseV1, WorkCallV1 } from "$shared/ipc/bindings";
const native = vi.hoisted(() => ({ call: vi.fn(), resources: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workCall: native.call, resourceCall: native.resources });
});
vi.mock("$shared/ipc/native-events", () => ({
  events: {
    workChanged: { listen: async () => () => {} },
    resourceChanged: { listen: async () => () => {} },
  },
}));
const profile = "00000000000000000000000001";
const snapshot: WorkEnvironmentSnapshot = {
  version: 1,
  profile,
  id: "environment",
  space: "space",
  title: "Work",
  revision: "1",
  lifecycle: "active",
  areas: [],
  elements: [],
  view: { revision: "1", x: 0, y: 0, zoom_milli: 1000, placements: [] },
};

test("attached context reads are bounded to two, use queries only, and discard late results after hide", async () => {
  const { WorkEnvironmentContext } = await import("../context.svelte");
  const resolvers: ((value: WorkResponseV1) => void)[] = [];
  native.call.mockReset();
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
    expect(call.kind).toBe("query");
    return new Promise<WorkResponseV1>((resolve) => resolvers.push(resolve));
  });
  const context = new WorkEnvironmentContext(profile);
  context.update({
    ...snapshot,
    elements: Array.from({ length: 40 }, (_, index) => ({
      id: `element-${index}`,
      area: null,
      reference: { kind: "objective" as const, objective: `objective-${index}` },
    })),
  });
  await context.start();
  await vi.waitFor(() => expect(native.call).toHaveBeenCalledTimes(2));
  resolvers[0]!({ version: 1, profile, reply: { kind: "error", error: "not_found" } });
  await vi.waitFor(() => expect(native.call).toHaveBeenCalledTimes(3));
  context.dispose();
  resolvers[1]!({ version: 1, profile, reply: { kind: "error", error: "not_found" } });
  await Promise.resolve();
  expect(context.objectives.size).toBe(0);
  expect(native.call).toHaveBeenCalledTimes(3);
});

test("note labels resolve only attached IDs without starting an objective", async () => {
  const { WorkEnvironmentContext } = await import("../context.svelte");
  native.call.mockReset();
  native.resources.mockReset();
  native.resources.mockResolvedValue({
    profile,
    response: {
      kind: "page",
      items: [
        {
          id: "note",
          revision: "1",
          title: "Saved findings",
          pinned: false,
          updated_at: "1",
          completed: null,
          due_date: null,
        },
      ],
      next: null,
    },
  });
  const context = new WorkEnvironmentContext(profile);
  context.update({
    ...snapshot,
    elements: [{ id: "element", area: null, reference: { kind: "resource", resource: "note" } }],
  });
  await context.start();
  await vi.waitFor(() => expect(context.notes[0]?.title).toBe("Saved findings"));
  expect(native.resources).toHaveBeenCalledExactlyOnceWith(profile, {
    kind: "resolve_notes",
    ids: ["note"],
  });
  expect(native.call).not.toHaveBeenCalled();
  context.dispose();
});
