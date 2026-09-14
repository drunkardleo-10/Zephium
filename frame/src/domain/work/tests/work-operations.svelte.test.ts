import { beforeEach, expect, test, vi } from "vitest";
import type { WorkOperationResponseV1, WorkOperationV1 } from "$shared/ipc/bindings";
const native = vi.hoisted(() => ({ begin: vi.fn(), status: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workOperation: native.begin, workOperationStatus: native.status });
});
const profile = "00000000000000000000000001";
const work = "00000000000000000000000002";
const input: WorkOperationV1 = {
  kind: "plan",
  request: { version: 1, work, expected_revision: "1" },
};
function response(
  operation: string,
  state: WorkOperationResponseV1["state"],
): WorkOperationResponseV1 {
  return { version: 1, profile, operation, state };
}
beforeEach(() => {
  vi.resetModules();
  native.begin.mockReset();
  native.status.mockReset();
});

test("reopening observes a lost operation response without repeating generation", async () => {
  const { WorkOperations } = await import("../work-operations.svelte");
  native.begin.mockRejectedValue(new Error("Lost response"));
  native.status.mockImplementation((_profile: string, _work: string, id: string) =>
    response(id, { kind: "pending", work }),
  );
  const changed = vi.fn(async () => {});
  const owner = new WorkOperations(profile, changed);
  await owner.begin(input);
  expect(owner.latest(work, "plan")?.state.kind).toBe("unknown");
  expect(owner.busy(work)).toBe(true);
  await owner.begin(input);
  expect(native.begin).toHaveBeenCalledTimes(1);
  owner.start();
  await owner.reconcile();
  expect(owner.latest(work, "plan")?.state.kind).toBe("pending");
  owner.stop();
  owner.start();
  await owner.reconcile();
  expect(native.begin).toHaveBeenCalledTimes(1);
  expect(native.status.mock.calls.every((call) => call[3] === false)).toBe(true);
  owner.stop();
});

test("status observation cannot race ahead of an unfinished admission", async () => {
  const { WorkOperations } = await import("../work-operations.svelte");
  let release!: (value: WorkOperationResponseV1) => void;
  native.begin.mockImplementation(
    () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  );
  native.status.mockImplementation((_profile: string, _work: string, id: string) =>
    response(id, { kind: "pending", work }),
  );
  const owner = new WorkOperations(profile, async () => {});
  owner.start();
  const pending = owner.begin(input);
  await vi.waitFor(() => expect(native.begin).toHaveBeenCalledTimes(1));
  await owner.reconcile();
  expect(native.status).not.toHaveBeenCalled();
  const id = owner.latest(work, "plan")!.id;
  release(response(id, { kind: "pending", work }));
  await pending;
  await owner.reconcile();
  expect(owner.latest(work, "plan")?.state.kind).toBe("pending");
  owner.stop();
});

test("missing native ownership after restart stays unknown and is never acknowledged as completion", async () => {
  const { WorkOperations } = await import("../work-operations.svelte");
  native.begin.mockImplementation((_profile: string, id: string) =>
    response(id, { kind: "pending", work }),
  );
  native.status.mockImplementation((_profile: string, _work: string, id: string) =>
    response(id, { kind: "unknown" }),
  );
  const owner = new WorkOperations(profile, async () => {});
  await owner.begin(input);
  owner.start();
  await owner.reconcile();
  expect(owner.latest(work, "plan")?.state.kind).toBe("unknown");
  expect(native.begin).toHaveBeenCalledTimes(1);
  expect(native.status.mock.calls.every((call) => call[3] === false)).toBe(true);
  owner.stop();
});

test("unknown public research reconciles the same operation without a new command or dispatch", async () => {
  const { WorkOperations } = await import("../work-operations.svelte");
  const input: WorkOperationV1 = {
    kind: "read_public",
    command: {
      version: 1,
      work,
      expected_revision: "1",
      command: "00000000000000000000000003",
      intent: {
        kind: "read_public",
        scope: { provider: "open_ai", model: "gpt-5.6-luna", query: "Find public sources" },
        limits: {
          model_tokens: 147456,
          cost_micro_usd: 100000,
          operations: 1,
          timeout_seconds: 180,
          max_workers: 1,
        },
      },
    },
  };
  native.begin.mockRejectedValue(new Error("Lost response"));
  native.status.mockImplementation((_profile: string, _work: string, id: string) =>
    response(id, { kind: "pending", work }),
  );
  const owner = new WorkOperations(profile, async () => {});
  await owner.begin(input);
  expect(owner.latest(work, "read_public")?.state.kind).toBe("unknown");
  await owner.begin(input);
  owner.start();
  await owner.reconcile();
  expect(native.begin).toHaveBeenCalledTimes(1);
  expect(native.begin.mock.calls[0]![2]).toEqual(input);
  expect(owner.latest(work, "read_public")?.state.kind).toBe("pending");
  expect(native.status.mock.calls.every((call) => call[1] === work && call[3] === false)).toBe(
    true,
  );
  owner.stop();
});
