import { planSteps, type ArtifactContent, type PlanStep } from "$shared/ui/data/Artifact/artifact";

/** At most this many steps become tasks at once; a longer plan stays whole in its block. */
const PLAN_CAP = 12;
export function resultPlan(content: ArtifactContent | undefined): PlanStep[] {
  return content ? planSteps(content).slice(0, PLAN_CAP) : [];
}
