import type { WorkExecutionFact } from "$domain/work";
import * as m from "$shared/i18n/messages";

type Step = NonNullable<WorkExecutionFact["steps"]>[number];

/** The agent's latest line for people, from the most recent turn that said something. */
export function agentLine(execution: WorkExecutionFact): string | null {
  const steps = execution.steps ?? [];
  for (let index = steps.length - 1; index >= 0; index--) {
    const step = steps[index]!;
    if (step.kind.kind === "turn" && step.note) return step.note;
  }
  return null;
}

/** One short label per settled step; never raw model or page text. */
export function stepLabel(step: Step): string {
  const kind = step.kind;
  switch (kind.kind) {
    case "search":
      return step.note ?? m.work_step_searched({ query: kind.query });
    case "read":
      return m.work_step_read({ host: host(kind.url) });
    case "discover":
      return step.note ?? m.work_step_browsed({ query: kind.query });
    case "publish":
      return step.note ?? m.work_step_published();
    case "ask":
      return kind.prompt;
    case "finish":
      return m.work_step_finished();
    case "turn":
      return step.note ?? "";
  }
}

function host(url: string): string {
  try {
    return new URL(url).host || url;
  } catch {
    return url;
  }
}

/** Whether an execution ran under the routine agent grant. */
export function isAgentExecution(execution: WorkExecutionFact): boolean {
  return execution.spec.nodes.some((node) => node.capability.kind === "agent");
}
