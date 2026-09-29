import { describe, expect, test } from "vitest";
import { PERSONAS, WORKFLOWS, composed, recentWorkflows, workflowOf } from "../lib/start/workflows";

const skills = import.meta.glob<string>("../../../../../crates/zephium-app/skills/*/SKILL.md", {
  query: "?raw",
  import: "default",
  eager: true,
});
const skill = (name: string) =>
  skills[`../../../../../crates/zephium-app/skills/${name}/SKILL.md`] ?? "";

describe("workflows", () => {
  test("each is a built-in skill, and every role has at least two", () => {
    for (const flow of WORKFLOWS) expect(skill(flow.skill)).toContain(`name: ${flow.skill}\n`);
    for (const persona of PERSONAS)
      expect(WORKFLOWS.filter((flow) => flow.persona === persona.id).length).toBeGreaterThan(1);
    expect(new Set(WORKFLOWS.map((flow) => flow.skill)).size).toBe(WORKFLOWS.length);
  });

  test("a workflow that needs an input asks one question, and one that needs none asks nothing", () => {
    for (const flow of WORKFLOWS) {
      expect(flow.request().trim()).toBe(flow.request());
      expect(!!flow.ask()).toBe(flow.input !== "none");
    }
  });

  test("its request takes the input at its end", () => {
    const explain = WORKFLOWS.find((flow) => flow.skill === "explain-code")!;
    const day = WORKFLOWS.find((flow) => flow.skill === "plan-my-day")!;
    expect(composed(explain, " ~/Dev/app ")).toBe("Explain how this code works: ~/Dev/app");
    expect(composed(explain)).toBe("Explain how this code works:");
    expect(composed(day, "the launch first")).toBe("Plan my day. the launch first");
  });

  test("an earlier request is known by its workflow's words or by the gallery's own request", () => {
    expect(workflowOf("Plan my day")?.skill).toBe("plan-my-day");
    expect(workflowOf("Map the market for AI scribes")?.skill).toBe("market-map");
    expect(workflowOf("Help me prepare for my exam in 6.006 on 16 October")?.skill).toBe(
      "exam-prep",
    );
    expect(workflowOf("Make a moodboard for warm brutalism")?.skill).toBe("moodboard");
    expect(workflowOf("Who wrote Dune?")).toBeNull();
    expect(workflowOf("  ")).toBeNull();
  });

  test("for you: the latest works' workflows, newest first, each once", () => {
    const works = [
      { requests: ["Plan my day"], touched_ms: "100" },
      {
        requests: ["Who wrote Dune?", "Research how competitors price issue trackers"],
        touched_ms: "300",
      },
      { requests: ["Plan my day"], touched_ms: "400" },
      { requests: ["Make a moodboard for calm fintech"], touched_ms: "200" },
      { requests: ["Triage my inbox"], touched_ms: "50" },
    ];
    expect(recentWorkflows(works).map((flow) => flow.skill)).toEqual([
      "plan-my-day",
      "pricing-research",
      "moodboard",
    ]);
    expect(recentWorkflows([])).toEqual([]);
  });
});
