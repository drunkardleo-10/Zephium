import { beforeEach, expect, test, vi } from "vitest";
import type {
  ResourceCall_Deserialize as Call,
  ResourceRecord_Serialize as Record,
  ResourceReply_Serialize as Reply,
} from "$shared/ipc/bindings";
const host = vi.hoisted(() => ({
  call: vi.fn(),
  listener: null as
    null | ((event: { payload: { profile: string; id: string; revision: string } }) => void),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ resourceCall: host.call });
});
vi.mock("$shared/ipc/native-events", () => ({
  events: {
    resourceChanged: {
      listen: (listener: typeof host.listener) => {
        host.listener = listener;
        return Promise.resolve(() => {
          host.listener = null;
        });
      },
    },
  },
}));
const profile = "00000000000000000000000001";
const record: Record = {
  id: "00000000000000000000000002",
  revision: "1",
  created_at: "100",
  updated_at: "100",
  trashed: false,
  draft: {
    title: "Original",
    pinned: false,
    related: [],
    content: { kind: "task", description: "", completed: false, due_date: null },
  },
};
function reply(response: Reply["response"]): Reply {
  return { profile, response };
}
beforeEach(() => {
  vi.resetModules();
  host.call.mockReset();
  host.listener = null;
});
function normal(_profile: string, call: Call): Promise<Reply> {
  if (call.kind === "list") return Promise.resolve(reply({ kind: "page", items: [], next: null }));
  if (call.kind === "get")
    return Promise.resolve(reply({ kind: "record", record: structuredClone(record) }));
  if (call.kind === "acknowledge") return Promise.resolve(reply({ kind: "acknowledged" }));
  throw new Error("Unexpected mutation");
}
test("coalesces edits made during a save against the newly committed revision", async () => {
  const { ResourceSession } = await import("../resources.svelte");
  host.call.mockImplementation(normal);
  const session = new ResourceSession(profile, "task");
  await session.start();
  await session.open(record.id);
  let release!: (reply: Reply) => void;
  let writes = 0;
  host.call.mockImplementation((scope: string, call: Call) => {
    if (call.kind !== "mutate") return normal(scope, call);
    writes++;
    const intent = call.command.intent;
    if (intent.kind !== "replace") throw new Error("Expected replace");
    const saved = reply({
      kind: "applied",
      request_id: call.command.request_id,
      applied_revision: String(writes + 1),
      record: { ...record, revision: String(writes + 1), draft: intent.draft },
    });
    if (writes === 1)
      return new Promise<Reply>((resolve) => {
        release = resolve;
      }).then(() => saved);
    expect(intent.expected_revision).toBe("2");
    return Promise.resolve(saved);
  });
  session.edit({ title: "First edit" });
  const pending = session.flush();
  await vi.waitFor(() => expect(writes).toBe(1));
  session.edit({ title: "Second edit" });
  host.listener?.({ payload: { profile, id: record.id, revision: "2" } });
  expect(session.saveState).not.toBe("conflict");
  release(reply({ kind: "acknowledged" }));
  await pending;
  expect(writes).toBe(2);
  expect(session.draft?.title).toBe("Second edit");
  expect(session.record?.revision).toBe("3");
  expect(session.saveState).toBe("saved");
  session.stopObserving();
});
test("reconciles an unknown creation with its original request identity", async () => {
  const { ResourceSession } = await import("../resources.svelte");
  host.call.mockImplementation(normal);
  const session = new ResourceSession(profile, "task");
  await session.start();
  let request = "";
  let writes = 0;
  host.call.mockImplementation((scope: string, call: Call) => {
    if (call.kind !== "mutate") return normal(scope, call);
    writes++;
    if (writes === 1) {
      request = call.command.request_id;
      return Promise.reject(new Error("lost reply"));
    }
    expect(call.command.request_id).toBe(request);
    return Promise.resolve(
      reply({ kind: "applied", request_id: request, applied_revision: "1", record }),
    );
  });
  await session.create("New task");
  expect(session.saveState).toBe("unknown");
  await session.retry();
  expect(session.record?.id).toBe(record.id);
  expect(session.saveState).toBe("saved");
  session.stopObserving();
});
test("retains drafts on stale revisions and refuses profile-mismatched read replies", async () => {
  const { ResourceSession } = await import("../resources.svelte");
  host.call.mockImplementation(normal);
  const session = new ResourceSession(profile, "task");
  await session.start();
  await session.open(record.id);
  host.call.mockImplementation((scope: string, call: Call) =>
    call.kind === "mutate"
      ? Promise.resolve(reply({ kind: "error", error: "conflict" }))
      : normal(scope, call),
  );
  session.edit({ title: "Keep this draft" });
  expect(await session.flush()).toBe(false);
  expect(session.saveState).toBe("conflict");
  expect(session.draft?.title).toBe("Keep this draft");
  session.stopObserving();
  const other = new ResourceSession("00000000000000000000000003", "task");
  await other.start();
  await other.open(record.id);
  expect(other.record).toBeNull();
  other.stopObserving();
});

test("retrying an unknown save preserves edits made after its original payload", async () => {
  const { ResourceSession } = await import("../resources.svelte");
  host.call.mockImplementation(normal);
  const session = new ResourceSession(profile, "task");
  await session.start();
  await session.open(record.id);
  let writes = 0;
  let original = "";
  host.call.mockImplementation((scope: string, call: Call) => {
    if (call.kind !== "mutate") return normal(scope, call);
    writes++;
    const intent = call.command.intent;
    if (intent.kind !== "replace") throw new Error("replace");
    if (writes === 1) {
      original = call.command.request_id;
      return Promise.reject(new Error("lost reply"));
    }
    if (writes === 2) {
      expect(call.command.request_id).toBe(original);
      expect(intent.draft.title).toBe("First");
    }
    const revision = writes === 2 ? "2" : "3";
    return Promise.resolve(
      reply({
        kind: "applied",
        request_id: call.command.request_id,
        applied_revision: revision,
        record: { ...record, revision, draft: intent.draft },
      }),
    );
  });
  session.edit({ title: "First" });
  await session.flush();
  expect(session.saveState).toBe("unknown");
  session.edit({ title: "Later local edit" });
  await session.retry();
  await session.flush();
  expect(session.draft?.title).toBe("Later local edit");
  expect(session.record?.draft.title).toBe("Later local edit");
  session.stopObserving();
});

test("autosave waits for idle, coalesces notifications, and stops timers when hidden", async () => {
  vi.useFakeTimers();
  const { ResourceSession } = await import("../resources.svelte");
  host.call.mockImplementation(normal);
  const session = new ResourceSession(profile, "task");
  try {
    await session.start();
    await session.open(record.id);
    let writes = 0;
    host.call.mockImplementation((scope: string, call: Call) => {
      if (call.kind !== "mutate") return normal(scope, call);
      writes++;
      const intent = call.command.intent;
      if (intent.kind !== "replace") throw new Error("Expected replace");
      host.listener?.({ payload: { profile, id: record.id, revision: "2" } });
      return Promise.resolve(
        reply({
          kind: "applied",
          request_id: call.command.request_id,
          applied_revision: "2",
          record: { ...record, revision: "2", draft: intent.draft },
        }),
      );
    });
    session.edit({ title: "Typing" });
    await vi.advanceTimersByTimeAsync(700);
    session.edit({ title: "Typing continues" });
    await vi.advanceTimersByTimeAsync(999);
    expect(writes).toBe(0);
    const listsBefore = host.call.mock.calls.filter(([, call]) => call.kind === "list").length;
    await vi.advanceTimersByTimeAsync(101);
    expect(writes).toBe(1);
    expect(
      host.call.mock.calls.filter(([, call]) => call.kind === "list").length - listsBefore,
    ).toBe(1);
    session.edit({ title: "Typing continues" });
    await vi.advanceTimersByTimeAsync(1100);
    expect(writes).toBe(1);
    session.edit({ title: "Retained draft" });
    session.stopObserving();
    await vi.advanceTimersByTimeAsync(2000);
    expect(writes).toBe(1);
    expect(session.draft?.title).toBe("Retained draft");
  } finally {
    session.stopObserving();
    vi.useRealTimers();
  }
});

test("background saving leaves edits made during a slow write for the next idle interval", async () => {
  vi.useFakeTimers();
  const { ResourceSession } = await import("../resources.svelte");
  host.call.mockImplementation(normal);
  const session = new ResourceSession(profile, "task");
  try {
    await session.start();
    await session.open(record.id);
    let release!: () => void;
    let writes = 0;
    host.call.mockImplementation(async (scope: string, call: Call) => {
      if (call.kind !== "mutate") return normal(scope, call);
      writes++;
      const intent = call.command.intent;
      if (intent.kind !== "replace") throw new Error("Expected replace");
      if (writes === 1)
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      return reply({
        kind: "applied",
        request_id: call.command.request_id,
        applied_revision: String(writes + 1),
        record: { ...record, revision: String(writes + 1), draft: intent.draft },
      });
    });
    session.edit({ title: "First" });
    await vi.advanceTimersByTimeAsync(1000);
    expect(writes).toBe(1);
    session.edit({ title: "Still typing" });
    release();
    await vi.advanceTimersByTimeAsync(0);
    expect(session.saveState).toBe("unsaved");
    await vi.advanceTimersByTimeAsync(999);
    expect(writes).toBe(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(writes).toBe(2);
    expect(session.record?.draft.title).toBe("Still typing");
  } finally {
    session.stopObserving();
    vi.useRealTimers();
  }
});

test("hidden saved sessions release bodies and reopen the current native revision", async () => {
  const { ResourceSession } = await import("../resources.svelte");
  host.call.mockImplementation(normal);
  const session = new ResourceSession(profile, "task");
  await session.start();
  await session.open(record.id);
  session.stopObserving();
  expect(session.record).toBeNull();
  expect(session.draft).toBeNull();
  expect(session.items).toEqual([]);
  host.call.mockImplementation((scope: string, call: Call) =>
    call.kind === "get"
      ? Promise.resolve(
          reply({
            kind: "record",
            record: {
              ...record,
              revision: "2",
              draft: { ...record.draft, title: "Changed while hidden" },
            },
          }),
        )
      : normal(scope, call),
  );
  await session.start();
  expect(session.record?.revision).toBe("2");
  expect(session.draft?.title).toBe("Changed while hidden");
  session.edit({ title: "Local unsaved draft" });
  session.stopObserving();
  expect(session.draft?.title).toBe("Local unsaved draft");
  expect(session.record?.revision).toBe("2");
});
