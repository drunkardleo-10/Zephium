import { describe, expect, it } from "vitest";
import { bucketAt, gridLines, timeScale } from "../bars";

describe("time axis", () => {
  it("rounds up to a step a person reads at a glance", () => {
    expect(timeScale(40 * 60)).toEqual({ top: 2700, step: 900 });
    expect(timeScale(3600)).toEqual({ top: 3600, step: 1800 });
    expect(timeScale(5 * 3600)).toEqual({ top: 21600, step: 7200 });
    expect(timeScale(0)).toEqual({ top: 60, step: 60 });
  });

  it("draws grid lines up to the top", () => {
    expect(gridLines({ top: 3600, step: 1200 })).toEqual([1200, 2400, 3600]);
  });

  it("finds the bucket under the pointer", () => {
    expect(bucketAt(0, 240, 24)).toBe(0);
    expect(bucketAt(239.9, 240, 24)).toBe(23);
    expect(bucketAt(400, 240, 24)).toBe(23);
    expect(bucketAt(5, 0, 24)).toBe(-1);
  });
});
