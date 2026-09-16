/** Fixed-height windowing for the history list.
 *
 *  History scrolls years, so the list renders only the rows on screen between
 *  two spacers. Every row is one of two known heights, which keeps offsets a
 *  prefix sum and lookup a binary search — no measurement, no layout thrash.
 */

export type RowHeights = { day: number; visit: number };

/** Below this the list renders plainly; windowing buys nothing at that size. */
export const WINDOW_THRESHOLD = 200;

/** Rows drawn beyond each edge so a fast scroll does not reveal blank space. */
const OVERSCAN = 8;

export type Window = {
  /** Index of the first rendered row. */
  first: number;
  /** Index after the last rendered row. */
  last: number;
  /** Pixels of unrendered rows above and below. */
  before: number;
  after: number;
};

/** Running offset of each row, plus the total, so `offsets[i]` opens row `i`. */
export function offsetsOf(kinds: readonly ("day" | "visit")[], heights: RowHeights): number[] {
  const offsets = new Array<number>(kinds.length + 1);
  let running = 0;
  for (let index = 0; index < kinds.length; index += 1) {
    offsets[index] = running;
    running += kinds[index] === "day" ? heights.day : heights.visit;
  }
  offsets[kinds.length] = running;
  return offsets;
}

export function totalHeight(offsets: readonly number[]): number {
  return offsets.length ? (offsets[offsets.length - 1] ?? 0) : 0;
}

/** Index of the last row that opens at or before `offset`. */
export function rowAt(offsets: readonly number[], offset: number): number {
  let low = 0;
  let high = offsets.length - 2;
  while (low < high) {
    const middle = (low + high + 1) >> 1;
    if ((offsets[middle] ?? 0) <= offset) low = middle;
    else high = middle - 1;
  }
  return Math.max(0, low);
}

export function windowFor(offsets: readonly number[], scrollTop: number, viewport: number): Window {
  const count = Math.max(0, offsets.length - 1);
  if (count === 0) return { first: 0, last: 0, before: 0, after: 0 };
  const first = Math.max(0, rowAt(offsets, Math.max(0, scrollTop)) - OVERSCAN);
  const last = Math.min(count, rowAt(offsets, Math.max(0, scrollTop) + viewport) + 1 + OVERSCAN);
  return {
    first,
    last,
    before: offsets[first] ?? 0,
    after: totalHeight(offsets) - (offsets[last] ?? 0),
  };
}
