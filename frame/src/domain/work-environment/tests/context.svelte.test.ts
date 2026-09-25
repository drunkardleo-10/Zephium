import { expect, test, vi } from "vitest";
import type { WorkEnvironmentSnapshot, WorkResponseV1, WorkCallV1 } from "$shared/ipc/bindings";
const native = vi.hoisted(() => ({
  call: vi.fn(),
  resources: vi.fn(),
  notes: vi.fn(),
  activity: vi.fn(),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    workCall: native.call,
    resourceCall: native.resources,
    noteCall: native.notes,
    workActivity: native.activity,
  });
});
vi.mock("$shared/ipc/native-events", () => ({
  events: {
    workChanged: { listen: async () => () => {} },
    resourceChanged: { listen: async () => () => {} },
    notesChanged: { listen: async () => () => {} },
  },
}));
const profile = "00000000000000000000000001";
const limits = {
  model_tokens: 100,
  cost_micro_usd: 0,
  operations: 1,
  timeout_seconds: 60,
  max_workers: 1,
};
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
  native.notes.mockReset();
  native.notes.mockResolvedValue({
    profile,
    response: {
      kind: "record",
      record: {
        summary: {
          id: "note",
          revision: "1",
          title: "Saved findings",
          preview: "",
          pinned: false,
          trashed: false,
          editable: true,
          created_at: "1",
          modified_at: "1",
          path: "Saved findings.md",
        },
        markdown: "# Saved findings\n",
      },
    },
  });
  const context = new WorkEnvironmentContext(profile);
  context.update({
    ...snapshot,
    elements: [{ id: "element", area: null, reference: { kind: "resource", resource: "note" } }],
  });
  await context.start();
  await vi.waitFor(() => expect(context.notes[0]?.title).toBe("Saved findings"));
  expect(native.notes).toHaveBeenCalledExactlyOnceWith(profile, { kind: "get", id: "note" });
  expect(native.resources).not.toHaveBeenCalled();
  expect(native.call).not.toHaveBeenCalled();
  context.dispose();
});

test("an attached work's page frames arrive with its projection, not with the session", async () => {
  const { WorkEnvironmentContext } = await import("../context.svelte");
  const projection = {
    version: 1,
    interrupted: [],
    executions: [
      {
        id: "execution",
        approved_revision: "1",
        status: "completed",
        attempts: [],
        spec: { plan_revision: "1", limits, nodes: [] },
        artifacts: [],
        user_artifacts: [],
      },
    ],
    work: {
      schema_version: 2,
      profile,
      id: "objective",
      revision: "1",
      lifecycle: "active",
      objective: "Compare quiet keyboards",
      objective_revision: "1",
      context_revision: "1",
      objective_author: "user",
      questions: [],
      status: "plan_ready",
      plan: null,
    },
  };
  native.call.mockReset();
  native.call.mockResolvedValue({ version: 1, profile, reply: { kind: "projection", projection } });
  native.activity.mockReset();
  native.activity.mockResolvedValue({
    version: 1,
    profile,
    work: "objective",
    signals: [],
    pages: [
      {
        execution: "execution",
        attempt: "attempt",
        step: "read",
        url: "https://a.example/one",
        live: false,
        frame: { generation: 1, width: 640, height: 400 },
      },
    ],
    error: null,
  });
  const context = new WorkEnvironmentContext(profile);
  context.update({
    ...snapshot,
    elements: [
      { id: "element", area: null, reference: { kind: "objective", objective: "objective" } },
    ],
  });
  await context.start();
  await vi.waitFor(() => expect(context.pages.get("objective")).toHaveLength(1));
  expect(native.activity).toHaveBeenCalledExactlyOnceWith(profile, "objective");
  context.dispose();
  expect(context.pages.size).toBe(0);
});
