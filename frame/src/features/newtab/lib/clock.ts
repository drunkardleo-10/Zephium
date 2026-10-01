export type ClockFormat = "system" | "12h" | "24h";

/** The time split the way it is set: the figures carry the display size and
 *  the day period is typeset beside them, smaller, on whichever side the
 *  locale puts it. */
export type ClockFace = { time: string; period: string | null; periodLeading: boolean };

const faces = new Map<string, Intl.DateTimeFormat>();

function formatter(format: ClockFormat, locale: string | undefined): Intl.DateTimeFormat {
  const key = `${locale ?? ""}|${format}`;
  let found = faces.get(key);
  if (!found) {
    found = new Intl.DateTimeFormat(locale, {
      hour: "numeric",
      minute: "2-digit",
      ...(format === "system" ? {} : { hour12: format === "12h" }),
    });
    faces.set(key, found);
  }
  return found;
}

export function clockFace(at: Date, format: ClockFormat, locale?: string): ClockFace {
  const parts = formatter(format, locale).formatToParts(at);
  const period = parts.findIndex((part) => part.type === "dayPeriod");
  if (period < 0)
    return { time: parts.map((part) => part.value).join(""), period: null, periodLeading: false };
  // The spacing that joins the period to the figures goes with the period.
  const time = parts
    .filter(
      (part, index) =>
        index !== period && !(part.type === "literal" && Math.abs(index - period) === 1),
    )
    .map((part) => part.value)
    .join("")
    .trim();
  return {
    time,
    period: parts[period]!.value,
    periodLeading: parts.slice(0, period).every((part) => part.type === "literal"),
  };
}

export function clockFormat(value: string): ClockFormat {
  return value === "12h" || value === "24h" ? value : "system";
}

/** Milliseconds until the next minute begins, so a clock redraws on the turn
 *  rather than up to a minute late. */
export function untilNextMinute(now: number): number {
  return 60_000 - (now % 60_000);
}

/** The local calendar day as `YYYY-MM-DD`, the shape a task's due date takes. */
export function dayKey(at: Date): string {
  return `${at.getFullYear()}-${String(at.getMonth() + 1).padStart(2, "0")}-${String(at.getDate()).padStart(2, "0")}`;
}
