/**
 * The motion tokens, for motion that CSS cannot express on its own: a layout
 * change measured before and after, an element that has already left the
 * document. Durations and curves are read from tokens.css so a scripted
 * animation and a declared one can never drift apart.
 */

export type Duration = "instant" | "fast" | "base" | "slow" | "page";
export type Easing = "out" | "smooth" | "in-out" | "exit" | "emphasized" | "spring" | "snap";

const durations = new Map<Duration, number>();
const easings = new Map<Easing, string>();

function token(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/** Milliseconds for a named duration. */
export function duration(name: Duration): number {
  let value = durations.get(name);
  if (value === undefined) {
    const raw = token(`--motion-${name}`);
    value = raw.endsWith("ms") ? Number.parseFloat(raw) : Number.parseFloat(raw) * 1000;
    if (!Number.isFinite(value)) value = 0;
    durations.set(name, value);
  }
  return value;
}

/** A timing function the Web Animations API accepts, for a named curve. */
export function easing(name: Easing): string {
  let value = easings.get(name);
  if (value === undefined) {
    value = token(`--ease-${name}`).replace(/\s+/gu, " ") || "ease-out";
    easings.set(name, value);
  }
  return value;
}

/** The operating system's preference or the in-app one, whichever asks for less. */
export function reducedMotion(): boolean {
  return (
    window.matchMedia("(prefers-reduced-motion: reduce)").matches ||
    document.documentElement.dataset.reduceMotion === "true"
  );
}
