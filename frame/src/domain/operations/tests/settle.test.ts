import { afterEach, describe, expect, it, vi } from "vitest";
import { settle } from "../settle";
const wait = vi.hoisted(() => vi.fn());
vi.mock("../operations", () => ({ waitForDisposition: wait }));
afterEach(() => {
  vi.useRealTimers();
  wait.mockReset();
});
describe("operation settlement", () => {
  it("retains deferred and the exact native disposition", async () => {
    const disposition = {
      operation_id: "0000000000000001",
      outcome: "deferred",
      reason: "native_work_pending",
    };
    wait.mockResolvedValue({ state: "processed", disposition });
    await expect(
      settle(Promise.resolve({ accepted: true, operation_id: disposition.operation_id })),
    ).resolves.toEqual({ outcome: "deferred", disposition });
  });
  it("does not query the ledger after rejected admission", async () => {
    await expect(settle(Promise.resolve({ accepted: false, operation_id: null }))).resolves.toEqual(
      { outcome: "rejected", disposition: null },
    );
    expect(wait).not.toHaveBeenCalled();
  });
  it("bounds a hung admission and releases its timer", async () => {
    vi.useFakeTimers();
    const pending = settle(new Promise(() => {}), 50);
    await vi.advanceTimersByTimeAsync(50);
    await expect(pending).resolves.toEqual({ outcome: "failed", reason: "timeout" });
    expect(vi.getTimerCount()).toBe(0);
  });
});

it("retains the exact operation identity when observation times out", async () => {
  vi.useFakeTimers();
  wait.mockReturnValue(new Promise(() => {}));
  const pending = settle(
    Promise.resolve({ accepted: true, operation_id: "0000000000000002" }),
    100,
  );
  await vi.advanceTimersByTimeAsync(100);
  await expect(pending).resolves.toMatchObject({
    outcome: "failed",
    reason: "timeout",
    operation_id: "0000000000000002",
  });
});
it("does not accept a disposition for a different native operation", async () => {
  wait.mockResolvedValue({
    state: "processed",
    disposition: { operation_id: "0000000000000003", outcome: "applied" },
  });
  await expect(
    settle(Promise.resolve({ accepted: true, operation_id: "0000000000000002" })),
  ).resolves.toMatchObject({
    outcome: "failed",
    reason: "unresolved",
    operation_id: "0000000000000002",
  });
});
