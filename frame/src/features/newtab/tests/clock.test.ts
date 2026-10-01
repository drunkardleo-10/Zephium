import { describe, expect, it } from "vitest";
import { clockFace, clockFormat, dayKey, untilNextMinute } from "../lib/clock";

const evening = new Date(2026, 8, 28, 21, 7);
const morning = new Date(2026, 8, 28, 9, 41);

describe("clockFace", () => {
  it("sets the period apart from the figures in a 12-hour clock", () => {
    expect(clockFace(evening, "12h", "en-US")).toEqual({
      time: "9:07",
      period: "PM",
      periodLeading: false,
    });
  });

  it("has no period in a 24-hour clock", () => {
    expect(clockFace(evening, "24h", "en-GB")).toEqual({
      time: "21:07",
      period: null,
      periodLeading: false,
    });
  });

  it("keeps the period on the side the locale puts it", () => {
    const face = clockFace(morning, "12h", "ko-KR");
    expect(face.periodLeading).toBe(true);
    expect(face.period).not.toBeNull();
    expect(face.time).toBe("9:41");
  });

  it("follows the locale when the format is the system's", () => {
    expect(clockFace(evening, "system", "en-GB").period).toBeNull();
    expect(clockFace(evening, "system", "en-US").period).toBe("PM");
  });
});

describe("clockFormat", () => {
  it("reads unknown stored values as the system format", () => {
    expect(clockFormat("24h")).toBe("24h");
    expect(clockFormat("12-hour")).toBe("system");
  });
});

describe("untilNextMinute", () => {
  it("lands on the turn of the minute", () => {
    expect(untilNextMinute(120_000)).toBe(60_000);
    expect(untilNextMinute(119_999)).toBe(1);
  });
});

describe("dayKey", () => {
  it("is the local calendar day", () => {
    expect(dayKey(new Date(2026, 8, 28, 23, 59))).toBe("2026-09-28");
    expect(dayKey(new Date(2026, 0, 5, 0, 0))).toBe("2026-01-05");
  });
});
