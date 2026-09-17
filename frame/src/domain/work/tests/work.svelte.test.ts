import { beforeEach, expect, test, vi } from "vitest";
import type {
  WorkActivityResponseV1,
  WorkCallV1,
  WorkExecutionFact,
  WorkPageV1,
  WorkResponseV1,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";

const native = vi.hoisted(() => ({
  call: vi.fn(),
  activity: vi.fn(),
  close: null as null | (() => Promise<boolean>),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workCall: native.call, workActivity: native.activity });
});
vi.mock("$shared/ipc/native-events", () => ({
  events: { workChanged: { listen: async () => () => {} } },
}));
vi.mock("$shared/lib/close", () => ({
  registerCloseTask: (task: () => Promise<boolean>) => {
    native.close = task;
    return () => {
      native.close = null;
    };
  },
}));
const profile = "00000000000000000000000001";
const work = "00000000000000000000000002";
function projection(revision = "1", executions: WorkExecutionFact[] = []): WorkRuntimeProjection {
  return {
    version: 1,
    executions,
    interrupted: [],
    work: {
      schema_version: 2,
      id: work,
      profile,
      revision,
      lifecycle: "active",
      objective: "Investigate a dependency",
      objective_revision: "1",
      context_revision: "1",
      objective_author: "user",
      status: "draft",
      plan: null,
      questions: [],
    },
  };
}
function response(reply: WorkResponseV1["reply"]): WorkResponseV1 {
  return { version: 1, profile, reply };
}
function reads(
  call: WorkCallV1,
  revision = "1",
  executions: WorkExecutionFact[] = [],
): WorkResponseV1 {
  if (call.kind !== "query") throw new Error("Unexpected mutation");
  if (call.request.query.kind === "list") return response({ kind: "page", works: [], next: null });
  return response({ kind: "projection", projection: projection(revision, executions) });
}
beforeEach(() => {
  vi.resetModules();
  native.call.mockReset();
  native.activity.mockReset();
  native.close = null;
});

test("keeps a draft's basis through fresh projections and Browse/Work transitions", async () => {
  const { WorkSession } = await import("../work.svelte");
  let revision = "1";
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => reads(call, revision));
  const session = new WorkSession(profile);
  await session.start();
  await session.open(work);
  session.setDraft("objective", "My investigation");
  revision = "9007199254740993";
  await session.open(work);
  session.stopObserving();
  await session.start();
  expect(session.draft("objective")).toBe("My investigation");
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
    if (call.kind !== "author" || call.command.intent.kind !== "edit") return reads(call, revision);
    expect(call.command.intent.expected_revision).toBe("1");
    return response({ kind: "error", error: "conflict" });
  });
  expect(await session.saveDraft("objective", "My investigation")).toBe(false);
  expect(session.delivery).toBe("conflict");
  await session.open(work);
  expect(session.delivery).toBe("conflict");
  expect(await native.close?.()).toBe(false);
  session.dispose();
});

test("retains exact operands after a wrong-profile acknowledgement and clears the confirmed draft on replay", async () => {
  const { WorkSession } = await import("../work.svelte");
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => reads(call));
  const session = new WorkSession(profile);
  await session.start();
  await session.open(work);
  session.setDraft("objective", "Review the dependency");
  const sent: WorkCallV1[] = [];
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
    if (call.kind !== "author") return reads(call, "2");
    sent.push(structuredClone(call));
    const reply = response({
      kind: "authoring_applied",
      receipt: {
        command: call.command.command,
        work,
        applied_revision: "2",
        deleted: false,
      },
    });
    return sent.length === 1 ? { ...reply, profile: "00000000000000000000000003" } : reply;
  });
  expect(await session.saveDraft("objective", "Review the dependency")).toBe(false);
  expect(session.delivery).toBe("unknown");
  expect(session.pending).not.toBeNull();
  session.stopObserving();
  await session.start();
  expect(sent).toHaveLength(1);
  expect(await session.reconcile()).toBe(true);
  expect(sent[1]).toEqual(sent[0]);
  expect(session.pending).toBeNull();
  expect(session.draft("objective")).toBeUndefined();
  session.dispose();
});

test("rejects an acknowledgement for another Work without losing the pending command", async () => {
  const { WorkSession } = await import("../work.svelte");
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => reads(call));
  const session = new WorkSession(profile);
  await session.start();
  await session.open(work);
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
    if (call.kind !== "author") return reads(call);
    return response({
      kind: "authoring_applied",
      receipt: {
        command: call.command.command,
        work: "00000000000000000000000003",
        applied_revision: "2",
        deleted: false,
      },
    });
  });
  expect(await session.edit({ kind: "archive" })).toBe(false);
  expect(session.delivery).toBe("unknown");
  expect(session.pending).not.toBeNull();
  session.dispose();
});

test("does not roll back a precision-safe projection when a stale read arrives", async () => {
  const { WorkSession } = await import("../work.svelte");
  let revision = "9007199254740993";
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => reads(call, revision));
  const session = new WorkSession(profile);
  await session.start();
  await session.open(work);
  revision = "9007199254740992";
  expect(await session.open(work)).toBe(false);
  expect(session.projection?.work.revision).toBe("9007199254740993");
  session.dispose();
});

test("a home session with an unresolved Work draft cannot be evicted", async () => {
  const { WorkSession } = await import("../work.svelte");
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => reads(call));
  const session = new WorkSession(profile);
  await session.start();
  await session.open(work);
  session.setDraft("objective", "Keep this draft");
  session.back();
  expect(session.canRelease).toBe(false);
  await session.open(work);
  session.discardDrafts();
  session.back();
  expect(session.canRelease).toBe(true);
  session.dispose();
});

test("page frames survive a settled run, an overtaking read, and a return to Work", async () => {
  const { WorkSession } = await import("../work.svelte");
  const limits = {
    model_tokens: 100,
    cost_micro_usd: 0,
    operations: 1,
    timeout_seconds: 60,
    max_workers: 1,
  };
  const settled: WorkExecutionFact = {
    id: "execution",
    approved_revision: "1",
    status: "completed",
    attempts: [],
    spec: {
      plan_revision: "1",
      limits,
      nodes: [{ node: "node", parent: null, capability: { kind: "synthesize" }, limits }],
    },
    artifacts: [],
    user_artifacts: [],
  };
  const page: WorkPageV1 = {
    execution: "execution",
    attempt: "attempt",
    step: "read-1",
    url: "https://shop.example/p/1",
    live: false,
    frame: { generation: 2, width: 640, height: 400 },
  };
  const waiting: ((value: WorkActivityResponseV1) => void)[] = [];
  native.call.mockImplementation((_profile: string, call: WorkCallV1) =>
    reads(call, "1", [settled]),
  );
  native.activity.mockImplementation(
    () => new Promise<WorkActivityResponseV1>((resolve) => waiting.push(resolve)),
  );
  const session = new WorkSession(profile);
  await session.start();
  // A run that has already finished still owns pages, so every open reads them.
  await session.open(work);
  await session.open(work);
  expect(waiting).toHaveLength(2);
  // The second open overtook the first; the first answer is still about this
  // work, so its frames are kept rather than thrown away.
  waiting.shift()!({ version: 1, profile, work, signals: [], pages: [page], error: null });
  await expect.poll(() => session.pages.length).toBe(1);
  // Leaving Work unmounts the workspace; the frames outlive it.
  session.stopObserving();
  expect(session.pages).toEqual([page]);
  await session.start();
  expect(session.pages).toEqual([page]);
  // Another objective starts with a clean canvas.
  await session.open("00000000000000000000000009");
  expect(session.pages).toEqual([]);
  session.dispose();
});
