import type { WorkExecutionFact, WorkStepFact } from "$shared/ipc/bindings";
import type { BoardScene } from "./board-fixtures";
import { fixScene } from "./parts-look";

/** The fix-the-bug run mid-way, with a browser and a research part beside GitHub and Code: four helpers at work. */
export function fourHelpersScene(): BoardScene {
  const scene = fixScene("working");
  const projection = [...scene.objectives.values()][0]!;
  const run = projection.executions[0] as WorkExecutionFact & {
    parts: Record<string, unknown>[];
  };
  const step = (id: string, part: string, kind: WorkStepFact["kind"], status = "running") =>
    ({ id, turn: 1, kind, part, status }) as WorkStepFact;
  run.steps = [
    ...(run.steps ?? [])
      .filter((each) => each.id !== "gh-4")
      .map((each) =>
        each.kind.kind === "edit_file" ? { ...each, kind: { ...each.kind, decision: true } } : each,
      ),
    step("gh-5", "github", { kind: "read", url: "https://github.com/cli/cli/pull/14543/files" }),
    step("docs-1", "docs", { kind: "read", url: "https://cli.github.com/manual/gh_repo_fork" }),
    step("web-1", "web", { kind: "search", query: "gh repo fork allow-forking organization" }),
  ];
  run.parts = [
    { ...run.parts[0]!, state: "running", summary: undefined },
    run.parts[1]!,
    {
      id: "docs",
      title: "Manual",
      helper: "browser",
      goal: "Read how gh documents forking",
      state: "running",
      started_ms: "1790620030000",
    },
    {
      id: "web",
      title: "Reports",
      helper: "research",
      goal: "Find other reports of the same flag",
      state: "running",
      started_ms: "1790620040000",
    },
  ];
  return scene;
}
