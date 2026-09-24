import { describe, expect, it } from "vitest";
import type {
  WorkCommandPolicyV1,
  WorkCommandRecordV1,
  WorkExecutionFact,
  WorkStepFact,
} from "$shared/ipc/bindings";
import { projection, snapshot } from "./environment-fixtures";
import { environmentItems } from "../lib/project-environment";
import { commandCards, commandRecord } from "../lib/project-environment-stage";

const policy = (
  cls: WorkCommandPolicyV1["class"],
  scope: WorkCommandPolicyV1["scope"],
): WorkCommandPolicyV1 => ({ class: cls, reason: "inspection", scope, root: "/Users/a/p" });
const command = (
  id: string,
  status: WorkStepFact["status"],
  local: WorkStepFact["local"],
  decision: boolean | null = null,
  evidence: string | null = null,
): WorkStepFact => ({
  id,
  turn: 1,
  kind: {
    kind: "run_command",
    cwd: "/Users/a/p",
    command: "npm test",
    timeout_secs: null,
    decision,
  },
  status,
  evidence,
  local,
});
const record = (id: string, exit: number): WorkCommandRecordV1 => ({
  id,
  node: "node",
  attempt: "attempt",
  command: {
    cwd: "/Users/a/p",
    command: "npm test",
    exit,
    signal: null,
    elapsed_ms: 4200,
    bytes: 12,
    digest: "a".repeat(64),
    text: "one\ntwo\nthree\nfour\n",
    truncated: false,
  },
});
const run = (steps: WorkStepFact[], records: WorkCommandRecordV1[] = []) =>
  ({ id: "run", steps, command_evidence: records }) as unknown as WorkExecutionFact;

describe("command cards", () => {
  it("runs with the live elapsed time and output tail", () => {
    const [card] = commandCards(
      run([
        command("s", "running", {
          policy: policy("read", "none"),
          output: { elapsed_ms: 2300, text: "a\nb\nc\nd", bytes: 7, truncated: false },
        }),
      ]),
    );
    expect(card).toEqual({
      id: "command:run:s",
      command: {
        line: "npm test",
        state: "running",
        elapsed_ms: 2300,
        tail: ["b", "c", "d"],
        reason: "Ran without asking",
      },
    });
    const [started] = commandCards(
      run([
        command("s", "running", {
          policy: policy("read", "none"),
          output: { text: "", bytes: 0, truncated: false },
        }),
      ]),
    );
    expect(started?.command.elapsed_ms).toBe(0);
  });

  it("waits for the person without a timer before the process starts", () => {
    const [card] = commandCards(
      run([command("s", "running", { policy: policy("ask", "command") })]),
    );
    expect(card?.command).toEqual({
      line: "npm test",
      state: "running",
      tail: [],
      reason: "waiting for you",
    });
  });

  it("settles on the record's exit and duration with the policy's reason", () => {
    const cards = commandCards(
      run(
        [
          command("read", "succeeded", { policy: policy("read", "none") }, null, "r1"),
          command("grant", "succeeded", { policy: policy("write", "folder") }, true, "r2"),
          command("held", "succeeded", { policy: policy("write", "none") }, null, "r3"),
          command("asked", "failed", { policy: policy("ask", "command") }, true, "r4"),
        ],
        [record("r1", 0), record("r2", 0), record("r3", 0), record("r4", 2)],
      ),
    );
    expect(cards.map((card) => card.command.reason)).toEqual([
      "Ran without asking",
      "Allowed for this folder",
      "Allowed for this folder",
      "You approved",
    ]);
    expect(cards[3]?.command).toMatchObject({
      state: "exit",
      exit: 2,
      elapsed_ms: 4200,
      tail: ["two", "three", "four"],
      record: "r4",
    });
    const objectives = new Map([
      ["objective", { ...projection, executions: [run([], [record("r4", 2)])] }],
    ]);
    expect(commandRecord(objectives, "r4")?.command.exit).toBe(2);
  });

  it("keeps a declined command off the canvas", () => {
    expect(
      commandCards(run([command("s", "failed", { policy: policy("ask", "command") }, false)])),
    ).toEqual([]);
  });
});

describe("link titles", () => {
  const url = "https://example.com/guide?utm_source=zephium";
  const read = (status: WorkStepFact["status"], title: string | null): WorkStepFact => ({
    id: `read-${status}`,
    turn: 1,
    kind: { kind: "read", url: "https://example.com/guide" },
    status,
    local: { page_title: title },
  });
  const linked = {
    ...snapshot,
    elements: [{ id: "link", area: null, reference: { kind: "link" as const, url, title: "" } }],
  };
  const items = (steps: WorkStepFact[]) =>
    environmentItems(
      linked,
      [],
      [],
      new Map([["objective", { ...projection, executions: [run(steps)] }]]),
    );

  it("names the card after the page a successful read observed, keeping the host below", () => {
    const [item] = items([read("succeeded", "The Guide")]);
    expect(item?.title).toBe("The Guide");
    expect(item?.detail).toBe("https://example.com");
    expect(linked.elements[0]?.reference.title).toBe("");
  });

  it("falls back to the host without a successful titled read", () => {
    expect(items([read("failed", "Nope")])[0]?.title).toBe("example.com");
    expect(items([read("succeeded", null)])[0]?.title).toBe("example.com");
  });
});
