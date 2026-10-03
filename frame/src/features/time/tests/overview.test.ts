import { describe, expect, it } from "vitest";
import { localDay } from "$domain/time";
import { columns, comparison, dailyAverage, totals } from "../lib/overview";

const bucket = (browse: number, work = 0) => ({ browse, work });

describe("overview", () => {
  it("adds the web and Work together", () => {
    const buckets = [bucket(60, 30), bucket(0), bucket(120)];
    expect(totals(buckets)).toEqual({ browse: 180, work: 30, all: 210 });
    expect(columns(buckets)).toEqual([90, 0, 120]);
  });

  it("compares a finished day with the day before and a running one with nothing", () => {
    const now = new Date(2026, 9, 3, 15);
    const today = { span: "day" as const, start: localDay(now) };
    const yesterday = { span: "day" as const, start: today.start - 1 };
    const current = totals([bucket(3600)]);
    expect(comparison(today, current, bucket(7200), now)).toBeNull();
    expect(comparison(yesterday, current, bucket(7200), now)).toBe(-3600);
    expect(comparison(yesterday, current, bucket(0), now)).toBeNull();
  });

  it("compares a running week by its daily average", () => {
    // Saturday afternoon: six days of a Monday week have begun.
    const now = new Date(2026, 9, 3, 15);
    const week = { span: "week" as const, start: localDay(now) - 5 };
    const current = totals([bucket(6 * 3600)]);
    expect(dailyAverage(week, current, now)).toBe(3600);
    expect(comparison(week, current, bucket(7 * 7200), now)).toBe(-3600);
  });
});
