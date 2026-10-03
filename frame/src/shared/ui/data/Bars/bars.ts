/** Steps a time axis may count in, in seconds: from a minute to six hours. */
const STEPS = [60, 300, 600, 900, 1800, 3600, 7200, 10800, 21600, 43200];

export interface Scale {
  /** The top of the axis, a whole number of steps. */
  top: number;
  step: number;
}

/** The smallest axis of at most `lines` steps that holds `peak`. */
export function timeScale(peak: number, lines = 3): Scale {
  const value = Math.max(0, peak);
  for (const step of STEPS) {
    if (value <= step * lines) return { top: Math.max(step, Math.ceil(value / step) * step), step };
  }
  const step = STEPS[STEPS.length - 1] ?? 43200;
  return { top: Math.ceil(value / step) * step, step };
}

/** Grid lines above the baseline, lowest first. */
export function gridLines(scale: Scale): number[] {
  const lines: number[] = [];
  for (let value = scale.step; value <= scale.top + 0.5; value += scale.step) lines.push(value);
  return lines;
}

/** The bucket under an x offset across a plot of `width` with `count` bars. */
export function bucketAt(x: number, width: number, count: number): number {
  if (width <= 0 || count <= 0) return -1;
  return Math.min(count - 1, Math.max(0, Math.floor((x / width) * count)));
}
