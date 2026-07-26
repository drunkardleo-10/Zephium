const NIGHT = "Working late";
const MORNING = "Good morning";
const AFTERNOON = "Good afternoon";
const EVENING = "Good evening";

/**
 * The new tab greeting. Kept framework-free and deterministic in its input so
 * it can be translated from one catalog and asserted without a clock stub.
 */
export function greetingFor(at: Date): string {
  const hour = at.getHours();
  if (!Number.isInteger(hour)) return MORNING;
  if (hour < 5) return NIGHT;
  if (hour < 12) return MORNING;
  if (hour < 18) return AFTERNOON;
  return EVENING;
}
