import { describe, expect, it } from "vitest";
import type { WorkCommandRecordV1, WorkExecutionFact, WorkStepFact } from "$shared/ipc/bindings";
import { commandClassLabel, commandSummary, pendingCommand } from "../lib/local-steps";
const step = (
  scope: "none" | "folder" | "command",
  decision: boolean | null = null,
): WorkStepFact => ({
  id: "step",
  turn: 1,
  kind: {
    kind: "run_command",
    cwd: "/Users/a/project",
    command: "pwd",
    timeout_secs: null,
    decision,
  },
  status: "running",
  usage: null,
  artifacts: [],
  evidence: null,
  note: null,
  measurements: null,
  local: {
    policy: {
      class: scope === "none" ? "read" : scope === "folder" ? "write" : "ask",
      reason: "inspection",
      scope,
      root: "/Users/a/project",
    },
  },
});
const execution = (steps: WorkStepFact[]) => ({ steps, status: "running" }) as WorkExecutionFact;
describe("command review projection", () => {
  it("only presents an unanswered approval, never an automatically running command", () => {
    expect(pendingCommand(execution([step("none")]))).toBeUndefined();
    expect(pendingCommand(execution([step("folder", true)]))).toBeUndefined();
    expect(pendingCommand(execution([step("command", false)]))).toBeUndefined();
    expect(pendingCommand(execution([{ ...step("folder"), status: "failed" }]))).toBeUndefined();
    expect(pendingCommand(execution([step("folder")]))?.id).toBe("step");
  });
  it("explains the approval class", () => {
    expect(commandClassLabel(step("none"))).toBe("Ran without asking");
    expect(commandClassLabel(step("folder", true))).toBe("Allowed for this folder");
    expect(commandClassLabel(step("command", true))).toBe("You approved");
  });
  it("reports exit codes and signals without treating zero as missing", () => {
    const record: WorkCommandRecordV1 = {
      id: "record",
      node: "node",
      attempt: "attempt",
      command: {
        cwd: "/Users/a/project",
        command: "pwd",
        exit: 0,
        signal: null,
        elapsed_ms: 1234,
        bytes: 0,
        digest: "a".repeat(64),
        text: "",
        truncated: false,
      },
    };
    expect(commandSummary(record)).toBe("Exit 0 · 1.2 s");
    record.command.exit = null;
    record.command.signal = 15;
    expect(commandSummary(record)).toBe("Signal 15 · 1.2 s");
  });
});
