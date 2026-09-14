import { beforeEach, expect, test, vi } from "vitest";
import type { WorkCallV1, WorkEnvironmentSnapshot, WorkResponseV1 } from "$shared/ipc/bindings";
const native = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workCall: native.call });
});
vi.mock("$shared/ipc/native-events", () => ({
  events: { workEnvironmentChanged: { listen: async () => () => {} } },
}));
vi.mock("$shared/lib/close", () => ({ registerCloseTask: () => () => {} }));
const profile = "00000000000000000000000001";
const id = "00000000000000000000000002";
const space = "00000000000000000000000003";
const element = "00000000000000000000000004";
function snapshot(revision = "1", viewRevision = "1"): WorkEnvironmentSnapshot {
  return {
    version: 1,
    id,
    profile,
    space,
    title: "A manual Work",
    lifecycle: "active",
    revision,
    elements: [],
    areas: [],
    view: { revision: viewRevision, x: 0, y: 0, zoom_milli: 1000, placements: [] },
  };
}
function response(reply: WorkResponseV1["reply"]): WorkResponseV1 {
  return { version: 1, profile, reply };
}
function page(selected: string | null = id) {
  return response({
    kind: "environment",
    reply: { kind: "page", works: [], next: null, selected },
  });
}
beforeEach(() => {
  vi.resetModules();
  native.call.mockReset();
});

test("reopens persisted selection without creating an objective or a second Work", async () => {
  const { WorkEnvironmentSession } = await import("../environment.svelte");
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
    if (call.kind !== "environment") throw new Error("No objective calls expected");
    if (call.request.kind === "list") return page();
    expect(call.request).toEqual({ kind: "open", id });
    return response({ kind: "environment", reply: { kind: "snapshot", snapshot: snapshot() } });
  });
  const session = new WorkEnvironmentSession(profile, space);
  await session.start("Untitled Work");
  expect(session.snapshot?.id).toBe(id);
  expect(native.call).toHaveBeenCalledTimes(2);
  session.dispose();
});

test("creates an empty persistent environment only after a confirmed empty list", async () => {
  const { WorkEnvironmentSession } = await import("../environment.svelte");
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
    if (call.kind !== "environment") throw new Error("No objective calls expected");
    if (call.request.kind === "list") return page(null);
    if (call.request.kind !== "command") throw new Error("Expected creation");
    expect(call.request.intent).toEqual({ kind: "create", space, title: "My Work" });
    return response({
      kind: "environment",
      reply: {
        kind: "applied",
        command: call.request.command,
        applied_revision: "1",
        applied_view_revision: "1",
        replayed: false,
        snapshot: snapshot(),
      },
    });
  });
  const session = new WorkEnvironmentSession(profile, space);
  await session.start("My Work");
  expect(session.snapshot?.elements).toEqual([]);
  expect(session.pending).toBeNull();
  session.dispose();
});

test("keeps unknown mutation operands across hide/show and retries their exact command", async () => {
  const { WorkEnvironmentSession } = await import("../environment.svelte");
  let saved = snapshot("9007199254740993", "7");
  let lost = true;
  const mutations: WorkCallV1[] = [];
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
    if (call.kind !== "environment") throw new Error("Unexpected call");
    if (call.request.kind === "list") return page();
    if (call.request.kind !== "command")
      return response({ kind: "environment", reply: { kind: "snapshot", snapshot: saved } });
    mutations.push(call);
    if (lost) {
      lost = false;
      return Promise.reject(new Error("lost reply"));
    }
    saved = {
      ...saved,
      revision: "9007199254740994",
      elements: [{ id: element, area: null, reference: { kind: "browser", tab: "retained-tab" } }],
    };
    return response({
      kind: "environment",
      reply: {
        kind: "applied",
        command: call.request.command,
        applied_revision: saved.revision,
        applied_view_revision: "7",
        replayed: true,
        snapshot: saved,
      },
    });
  });
  const session = new WorkEnvironmentSession(profile, space);
  await session.start("Untitled");
  expect(
    await session.edit({
      kind: "add",
      reference: { kind: "browser", tab: "retained-tab" },
      area: null,
    }),
  ).toBe(false);
  expect(session.delivery).toBe("unknown");
  const pending = session.pending;
  session.stopObserving();
  await session.start("Untitled");
  expect(mutations[0]).toEqual(mutations[1]);
  expect(pending?.kind === "command" ? pending.intent : null).toMatchObject({
    expected: "9007199254740993",
  });
  expect(session.snapshot?.elements[0]?.reference).toEqual({
    kind: "browser",
    tab: "retained-tab",
  });
  session.dispose();
});

test("checkpoints use view revision and retain local arrangement on conflict", async () => {
  const { WorkEnvironmentSession } = await import("../environment.svelte");
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
    if (call.kind !== "environment") throw new Error("Unexpected call");
    if (call.request.kind === "list") return page();
    if (call.request.kind !== "checkpoint")
      return response({
        kind: "environment",
        reply: { kind: "snapshot", snapshot: snapshot("91", "7") },
      });
    expect(call.request).toMatchObject({
      kind: "checkpoint",
      id,
      expected: "7",
      view: { revision: "7" },
    });
    expect(call.request).not.toHaveProperty("command");
    return response({ kind: "error", error: "conflict" });
  });
  const session = new WorkEnvironmentSession(profile, space);
  await session.start("Untitled");
  session.checkpoint({ ...snapshot().view, x: 42 });
  expect(await session.flushView()).toBe(false);
  expect(session.viewDraft?.view.x).toBe(42);
  expect(session.delivery).toBe("conflict");
  session.dispose();
});

test("rejects a cross-Space or wrong-command reply without claiming mutation completion", async () => {
  const { WorkEnvironmentSession } = await import("../environment.svelte");
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
    if (call.kind !== "environment") throw new Error("Unexpected call");
    if (call.request.kind === "list") return page();
    if (call.request.kind !== "command")
      return response({ kind: "environment", reply: { kind: "snapshot", snapshot: snapshot() } });
    return response({
      kind: "environment",
      reply: {
        kind: "applied",
        command: "wrong",
        applied_revision: "2",
        applied_view_revision: "1",
        replayed: false,
        snapshot: { ...snapshot("2"), space: "another-space" },
      },
    });
  });
  const session = new WorkEnvironmentSession(profile, space);
  await session.start("Untitled");
  expect(await session.edit({ kind: "rename", title: "Changed" })).toBe(false);
  expect(session.pending).not.toBeNull();
  expect(session.snapshot?.title).toBe("A manual Work");
  session.dispose();
});

test.each([false, true])(
  "unknown checkpoint retains exact identity and later gestures (remote advanced: %s)",
  async (advanced) => {
    const { WorkEnvironmentSession } = await import("../environment.svelte");
    const session = new WorkEnvironmentSession(profile, space);
    session.snapshot = snapshot("91", "7");
    const calls: WorkCallV1[] = [];
    native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
      calls.push(call);
      if (call.kind !== "environment" || call.request.kind !== "checkpoint")
        throw new Error("Only checkpoints expected");
      const request = call.request;
      if (calls.length === 1) throw new Error("lost receipt");
      const applied = String(BigInt(request.expected) + 1n);
      return response({
        kind: "environment",
        reply: {
          kind: "checkpointed",
          expected: request.expected,
          applied_view_revision: applied,
          replayed: calls.length === 2,
          snapshot: snapshot("91", advanced ? "10" : applied),
        },
      });
    });
    session.checkpoint({ ...snapshot().view, x: 42 });
    expect(await session.flushView()).toBe(false);
    const pending = session.pending;
    session.checkpoint({ ...snapshot().view, x: 84 });
    expect(await session.edit({ kind: "rename", title: "Blocked" })).toBe(false);
    expect(calls).toHaveLength(1);
    expect(session.pending).toBe(pending);
    expect(await session.retry()).toBe(true);
    expect(calls[0]).toEqual(calls[1]);
    expect(session.viewDraft?.view).toMatchObject({ revision: "8", x: 84 });
    if (advanced) {
      expect(session.delivery).toBe("conflict");
      expect(await session.flushView()).toBe(false);
      expect(await session.create("Blocked")).toBe(false);
      expect(calls).toHaveLength(2);
    } else {
      expect(await session.flushView()).toBe(true);
      expect(calls[2]).toMatchObject({
        request: { kind: "checkpoint", expected: "8", view: { revision: "8", x: 84 } },
      });
      expect(session.snapshot?.view.revision).toBe("9");
      expect(session.viewDraft).toBeNull();
    }
    session.dispose();
  },
);

test("mismatched checkpoint receipt retains unknown operands and draft", async () => {
  const { WorkEnvironmentSession } = await import("../environment.svelte");
  const session = new WorkEnvironmentSession(profile, space);
  session.snapshot = snapshot("91", "7");
  native.call.mockResolvedValue(
    response({
      kind: "environment",
      reply: {
        kind: "checkpointed",
        expected: "6",
        applied_view_revision: "8",
        replayed: true,
        snapshot: snapshot("91", "8"),
      },
    }),
  );
  session.checkpoint({ ...snapshot().view, x: 42 });
  expect(await session.flushView()).toBe(false);
  expect(session.delivery).toBe("unknown");
  expect(session.pending).toMatchObject({ kind: "checkpoint", expected: "7" });
  expect(session.viewDraft?.view.x).toBe(42);
  expect(session.snapshot.view.revision).toBe("7");
  session.dispose();
});

test("a checkpoint reply with a newer remote view cannot release a queued semantic mutation", async () => {
  const { WorkEnvironmentSession } = await import("../environment.svelte");
  const session = new WorkEnvironmentSession(profile, space);
  session.snapshot = snapshot("91", "7");
  let resolve!: (value: WorkResponseV1) => void;
  native.call.mockImplementation(
    () =>
      new Promise<WorkResponseV1>((done) => {
        resolve = done;
      }),
  );
  session.checkpoint({ ...snapshot().view, x: 42 });
  const editing = session.edit({ kind: "rename", title: "Must wait" });
  await vi.waitFor(() => expect(native.call).toHaveBeenCalledTimes(1));
  session.checkpoint({ ...snapshot().view, x: 84 });
  resolve(
    response({
      kind: "environment",
      reply: {
        kind: "checkpointed",
        expected: "7",
        applied_view_revision: "8",
        replayed: true,
        snapshot: snapshot("91", "10"),
      },
    }),
  );
  expect(await editing).toBe(false);
  expect(session.delivery).toBe("conflict");
  expect(session.viewDraft?.view.x).toBe(84);
  expect(native.call).toHaveBeenCalledTimes(1);
  session.dispose();
});
