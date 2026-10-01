import { expect, test } from "vitest";
import type { WorkHumanPageV1, WorkHumanPhaseV1 } from "$shared/ipc/bindings";
import { cardCountdown, heldPage, regionOf, sameRegion, MIN_REGION } from "../lib/work-human";

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

test("a region floors its size, keeps the 8192 extents, and stays inside the parent", () => {
  const parent = { width: 1200, height: 800 };
  expect(regionOf({ x: 300.4, y: 120.6, width: 640.2, height: 420.8 }, parent)).toEqual({
    x: 300,
    y: 121,
    width: 640,
    height: 421,
  });
  // The corner may sit at the content view's origin: Rust floors size alone.
  expect(regionOf({ x: 0, y: 0, width: MIN_REGION, height: MIN_REGION }, parent)).toEqual({
    x: 0,
    y: 0,
    width: MIN_REGION,
    height: MIN_REGION,
  });
  expect(regionOf({ x: 300, y: 120, width: MIN_REGION - 1, height: 400 }, parent)).toBeNull();
  expect(regionOf({ x: 300, y: 120, width: 400, height: MIN_REGION - 1 }, parent)).toBeNull();
  expect(regionOf({ x: -1, y: 120, width: 400, height: 400 }, parent)).toBeNull();
  const wide = { width: 10_000, height: 10_000 };
  expect(regionOf({ x: 7900, y: 120, width: 400, height: 400 }, wide)).toBeNull();
  expect(regionOf({ x: 120, y: 7900, width: 400, height: 400 }, wide)).toBeNull();
  expect(regionOf({ x: 7700, y: 7700, width: 400, height: 400 }, wide)).toEqual({
    x: 7700,
    y: 7700,
    width: 400,
    height: 400,
  });
  expect(regionOf({ x: 300, y: 120, width: 400, height: 720 }, parent)).toBeNull();
  expect(regionOf({ x: 300, y: 120, width: Number.NaN, height: 400 }, parent)).toBeNull();
});

test("an unchanged placement is not torn down to be rebuilt", () => {
  const region = { x: 300, y: 120, width: 640, height: 420 };
  expect(sameRegion(region, { ...region })).toBe(true);
  expect(sameRegion(region, { ...region, width: 641 })).toBe(false);
  expect(sameRegion(region, null)).toBe(false);
  expect(sameRegion(null, null)).toBe(false);
});
