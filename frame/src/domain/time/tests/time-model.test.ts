import { describe, expect, it } from "vitest";
import {
  breakMinutes,
  countdown,
  dateOfDay,
  elapsedBuckets,
  firstDayOfWeek,
  liveBucket,
  localDay,
  localMs,
  normalizeSite,
  reportCall,
  shiftPeriod,
  weekStart,
} from "../time-model";

describe("local time", () => {
  it("counts local days from the epoch the way native counts local hours", () => {
    const noon = new Date(2026, 9, 3, 12);
    expect(localMs(noon) % 86_400_000).toBe(12 * 3_600_000);
    expect(dateOfDay(localDay(noon)).getDate()).toBe(3);
  });

  it("starts weeks on the locale's first day", () => {
    // 2026-10-03 is a Saturday.
    const saturday = localDay(new Date(2026, 9, 3, 9));
    expect(dateOfDay(weekStart(saturday, 1)).getDay()).toBe(1);
    expect(dateOfDay(weekStart(saturday, 7)).getDay()).toBe(0);
    expect(weekStart(weekStart(saturday, 1), 1)).toBe(weekStart(saturday, 1));
    expect(firstDayOfWeek("not a locale tag at all")).toBe(1);
  });

  it("knows which bucket is now and how many have begun", () => {
    const now = new Date(2026, 9, 3, 14, 30);
    const day = { span: "day" as const, start: localDay(now) };
    expect(liveBucket(day, now)).toBe(14);
    expect(elapsedBuckets(day, now)).toBe(15);
    expect(liveBucket(shiftPeriod(day, -1), now)).toBe(-1);
    expect(elapsedBuckets(shiftPeriod(day, -1), now)).toBe(24);
    expect(elapsedBuckets(shiftPeriod(day, 1), now)).toBe(0);
  });

  it("asks for hours of a day and days of a week", () => {
    expect(reportCall({ span: "day", start: 10 })).toEqual({
      kind: "report",
      from_hour: 240,
      bucket_hours: 1,
      buckets: 24,
      site: null,
    });
    expect(reportCall({ span: "week", start: 7 }, "x.com")).toMatchObject({
      from_hour: 168,
      bucket_hours: 24,
      buckets: 7,
      site: "x.com",
    });
  });
});

describe("focus figures", () => {
  it("rests the way native schedules breaks", () => {
    expect(breakMinutes(25)).toEqual({ short: 5, long: 15 });
    expect(breakMinutes(50)).toEqual({ short: 10, long: 30 });
    expect(breakMinutes(90)).toEqual({ short: 18, long: 30 });
  });

  it("reads a countdown like a clock", () => {
    expect(countdown(1084)).toBe("18:04");
    expect(countdown(3729)).toBe("1:02:09");
    expect(countdown(-3)).toBe("0:00");
    expect(countdown(0.2)).toBe("0:01");
  });
});

describe("sites", () => {
  it("reduces what is typed to the host native matches", () => {
    expect(normalizeSite("https://www.YouTube.com/watch?v=1")).toBe("youtube.com");
    expect(normalizeSite(" x.com ")).toBe("x.com");
    expect(normalizeSite("mail.google.com/inbox")).toBe("mail.google.com");
    expect(normalizeSite("localhost")).toBeNull();
    expect(normalizeSite("192.168.1.1")).toBeNull();
    expect(normalizeSite("file:///etc")).toBeNull();
    expect(normalizeSite("")).toBeNull();
  });
});
