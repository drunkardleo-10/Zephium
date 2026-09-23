import { expect, test } from "vitest";
import type { WorkHumanPageV1, WorkHumanPhaseV1 } from "$shared/ipc/bindings";
import { cardCountdown, heldPage } from "../lib/work-human";

const page = (step: string, phase: WorkHumanPhaseV1, attempt = "attempt"): WorkHumanPageV1 => ({
  id: { attempt, step, generation: 3 },
  phase,
  reason: "sign_in",
  remaining_millis: 120_000,
  document_revision: "7",
  can_continue: true,
});

test("the furthest-along held page wins and a page the agent has back is not held", () => {
  expect(heldPage([page("a", "waiting_for_human"), page("b", "presented")])?.id.step).toBe("b");
  expect(heldPage([page("a", "reading"), page("b", "released")])).toBeNull();
  expect(heldPage([])).toBeNull();
});

test("the countdown appears only near the end of the wait", () => {
  expect(cardCountdown(120_000)).toBeNull();
  expect(cardCountdown(59_400)).toBe("60s left");
  expect(cardCountdown(12_100)).toBe("13s left");
  expect(cardCountdown(0)).toBeNull();
});
