import { describe, expect, test } from "vitest";
import { agentOf, changeCounts, computerRows, computerView, testsIn } from "../lib/parts/computer";
import { connectionRows, connectionView } from "../lib/parts/connection";
import { fixScene } from "./parts-look";

const projection = (stage: "done" | "working" | "handoff") =>
  [...fixScene(stage).objectives.values()][0]!;
const ids = (stage: "done" | "working" | "handoff", part: string) =>
  (projection(stage).executions[0]!.steps ?? [])
    .filter((step) => step.part === part)
    .map((step) => step.id);

describe("a computer part's view", () => {
  test("changes, commands, tests and what it read", () => {
    const view = computerView(projection("done"), ids("done", "code"));
    expect(view.folder).toBe("zephium");
    expect(view.files.map((f) => [f.name, f.folder, f.added, f.removed, f.state])).toEqual([
      ["duration.rs", "…/src/work", 1, 1, "done"],
      ["tests.rs", "…/src/work", 5, 0, "done"],
    ]);
    expect(view.commands.map((c) => [c.state, c.tests, c.ms])).toEqual([
      ["failed", { passed: 42, failed: 1 }, 4830],
      ["passed", { passed: 43, failed: 0 }, 3210],
    ]);
    expect(view.tests).toEqual({ passed: 43, failed: 0 });
    expect([view.reads, view.searches, view.working]).toEqual([2, 2, false]);
    expect(computerRows(view)).toBe(4);
  });

  test("a change waiting on the person and a command still running", () => {
    const view = computerView(projection("working"), ids("working", "code"));
    expect(view.files[0]!.state).toBe("waiting");
    const running = view.commands.at(-1)!;
    expect(running.state).toBe("running");
    expect(running.live).toContain("Compiling zephium-store");
    expect(view.working).toBe(true);
  });

  test("a hand-off names its agent and what it is editing", () => {
    const view = computerView(projection("handoff"), ids("handoff", "code"));
    const handoff = view.commands.at(-1)!;
    expect(handoff.agent).toBe("codex");
    expect(handoff.live).toBe("✎ duration.rs");
    expect(agentOf("claude -p --output-format stream-json -- 'x'")).toBe("claude");
    expect(agentOf("cargo test")).toBeNull();
  });

  test("diff counts and test summaries", () => {
    expect(changeCounts("@@ -1,2 +1,3 @@\n a\n-b\n+c\n+d\n")).toEqual([2, 1]);
    expect(testsIn("=== 2 failed, 10 passed in 1.2s ===")).toEqual({ passed: 10, failed: 2 });
    expect(testsIn("Tests:       1 failed, 7 passed, 8 total")).toEqual({ passed: 7, failed: 1 });
    expect(testsIn("      Tests  40 passed (40)")).toEqual({ passed: 40, failed: 0 });
    expect(testsIn("--- PASS: TestA\n--- FAIL: TestB")).toEqual({ passed: 1, failed: 1 });
    expect(testsIn("Ran 2 tests in 0.001s\n\nFAILED (failures=1)\n")).toEqual({
      passed: 1,
      failed: 1,
    });
    expect(testsIn("Ran 3 tests in 0.002s\n\nOK\n")).toEqual({ passed: 3, failed: 0 });
    expect(testsIn("Compiling")).toBeNull();
  });
});

describe("a connection part's view", () => {
  test("its calls as rows, and a write held for the person", () => {
    const done = connectionView(projection("done"), ids("done", "github"), { title: "GitHub" });
    expect(done.service).toBe("github");
    expect(done.calls.map((c) => [c.text, c.state])).toEqual([
      ["Read issue #10000", "done"],
      ["Listed 4 pull requests", "done"],
      ["Read checks on #14543", "done"],
    ]);
    expect(done.calls[0]!.detail).toContain("--allow-forking");
    expect(connectionRows(done)).toBe(3);
    const working = connectionView(projection("working"), ids("working", "github"), {
      title: "GitHub",
    });
    expect(working.calls.at(-1)).toMatchObject({
      text: "Comment on #10000 as ada",
      state: "waiting",
    });
  });
});
