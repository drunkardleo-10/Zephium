import { planSteps, type ArtifactContent, type PlanStep } from "$shared/ui/data/Artifact/artifact";
import type { CanvasItem } from "./canvas-model";

export type StepIcon = NonNullable<CanvasItem["step"]>["icon"];

/**
 * A closed keyword table, first match wins: travel words outrank the dates and
 * money they usually mention ("enter dates and a budget on Airbnb" is a stay).
 */
const ICONS: readonly (readonly [StepIcon, RegExp])[] = [
  [
    "flight",
    /\b(flights?|fares?|airlines?|airports?|plane|layovers?|nonstop|boarding|waw|sfo|krk|jfk|lax)\b/iu,
  ],
  ["entry", /\b(visas?|esta|entry|passports?|border|customs|immigration|cbp)\b/iu],
  [
    "stay",
    /\b(airbnb|stays?|rooms?|apartments?|hotels?|flats?|accommodation|lodging|rentals?)\b/iu,
  ],
  ["money", /(\$|€|£|\b(budget|costs?|prices?|payments?|pay|fees?)\b)/iu],
  ["dates", /\b(dates?|calendar|window|schedules?|deadlines?)\b/iu],
  ["document", /\b(applications?|apply|forms?|documents?|paperwork)\b/iu],
];
export function stepIcon(text: string): StepIcon {
  return ICONS.find(([, pattern]) => pattern.test(text))?.[0] ?? "check";
}

/** At most this many steps become cards; a longer plan stays whole in the lift. */
const PLAN_CAP = 12;
export function resultPlan(content: ArtifactContent | undefined): PlanStep[] {
  return content ? planSteps(content).slice(0, PLAN_CAP) : [];
}
/** A step card's id: its result's card and its place in the plan. */
export const stepId = (result: string, index: number) => `step:${result}:${index}`;
/** The result card a step card belongs to. */
export const stepResult = (id: string) =>
  id.startsWith("step:") ? id.slice(5, id.lastIndexOf(":")) : null;
