import { expect, test } from "vitest";
import {
  addDays,
  formatDay,
  inMonth,
  monthGrid,
  monthOf,
  nextWeek,
  nextWeekday,
  parseDay,
  shiftMonth,
  weekdayNames,
} from "../lib/task-calendar";

test("a day that parses must be the day it claims", () => {
  expect(formatDay(parseDay("2028-02-29")!)).toBe("2028-02-29");
  expect(parseDay("2027-02-29")).toBeNull();
  expect(parseDay("2026-13-01")).toBeNull();
  expect(parseDay("20260101")).toBeNull();
});

test("adding days crosses months, years and a leap day without drifting", () => {
  expect(addDays("2026-09-20", 1)).toBe("2026-09-21");
  expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
  expect(addDays("2028-02-28", 1)).toBe("2028-02-29");
  expect(addDays("2026-01-01", -1)).toBe("2025-12-31");
  // A date in a zone that shifts its clock this day is still one day later.
  expect(addDays("2026-03-29", 1)).toBe("2026-03-30");
});

test("the next named day is never today", () => {
  // 2026-09-20 is a Sunday.
  expect(nextWeekday("2026-09-20", 0)).toBe("2026-09-27");
  expect(nextWeekday("2026-09-20", 6)).toBe("2026-09-26");
  expect(nextWeek("2026-09-20", 1)).toBe("2026-09-21");
});

test("a month grid is six aligned weeks that start where the locale starts", () => {
  const grid = monthGrid(2026, 8, 1);
  expect(grid).toHaveLength(6);
  expect(grid.every((week) => week.length === 7)).toBe(true);
  expect(grid.flat()).toHaveLength(42);
  // Every row runs forward one day at a time across the whole grid.
  const flat = grid.flat();
  expect(flat.every((day, index) => index === 0 || day === addDays(flat[index - 1]!, 1))).toBe(
    true,
  );
  expect(flat).toContain("2026-09-01");
  expect(flat).toContain("2026-09-30");
  expect(inMonth("2026-09-01", 2026, 8)).toBe(true);
  expect(inMonth("2026-08-31", 2026, 8)).toBe(false);
  expect(weekdayNames(1)).toHaveLength(7);
});

test("stepping months wraps the year in both directions", () => {
  expect(shiftMonth(2026, 11, 1)).toEqual({ year: 2027, month: 0 });
  expect(shiftMonth(2026, 0, -1)).toEqual({ year: 2025, month: 11 });
  expect(monthOf("2026-09-20")).toEqual({ year: 2026, month: 8 });
});
