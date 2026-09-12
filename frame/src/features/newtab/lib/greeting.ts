import * as m from "$shared/i18n/messages";

/**
 * The new tab greeting. Kept framework-free and deterministic in its input so
 * it can be translated from one catalog and asserted without a clock stub.
 */
export function greetingFor(at: Date): string {
  const hour = at.getHours();
  if (!Number.isInteger(hour)) return m.greeting_morning();
  if (hour < 5) return m.greeting_night();
  if (hour < 12) return m.greeting_morning();
  if (hour < 18) return m.greeting_afternoon();
  return m.greeting_evening();
}
