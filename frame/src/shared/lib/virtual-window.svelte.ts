/** Windowed rendering for long lists.
 *
 *  Rows declare their height rather than being measured, which keeps offsets a
 *  prefix sum and the row under any scroll position a binary search: no layout
 *  reads, no measurement pass, nothing to invalidate. Lists whose rows are a
 *  known height — every list in this product so far — need nothing more.
 *
 *  A list of genuinely unknown-height rows needs measurement and scroll
 *  anchoring, which is the hard part of the problem and the reason to reach
 *  for a library instead of extending this.
 */

export type VirtualWindow = {
  /** Index of the first rendered row. */
  first: number;
  /** Index after the last rendered row. */
  last: number;
  /** Pixels of unrendered rows above and below the rendered slice. */
  before: number;
  after: number;
};

/** Rows drawn beyond each edge so a fast scroll does not reveal blank space. */
const DEFAULT_OVERSCAN = 8;

/** Below this many rows, plain rendering costs less than the spacers do. */
const DEFAULT_THRESHOLD = 200;

/** Running offset of each row, plus the total, so `offsets[i]` opens row `i`. */
export function offsetsOf(heights: readonly number[]): number[] {
  const offsets = new Array<number>(heights.length + 1);
  let running = 0;
  for (let index = 0; index < heights.length; index += 1) {
    offsets[index] = running;
    running += heights[index] ?? 0;
  }
  offsets[heights.length] = running;
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

export function windowFor(
  offsets: readonly number[],
  scrollTop: number,
  viewport: number,
  overscan = DEFAULT_OVERSCAN,
): VirtualWindow {
  const count = Math.max(0, offsets.length - 1);
  if (count === 0) return { first: 0, last: 0, before: 0, after: 0 };
  const top = Math.max(0, scrollTop);
  const first = Math.max(0, rowAt(offsets, top) - overscan);
  const last = Math.min(count, rowAt(offsets, top + viewport) + 1 + overscan);
  return {
    first,
    last,
    before: offsets[first] ?? 0,
    after: totalHeight(offsets) - (offsets[last] ?? 0),
  };
}

/** Reactive window over a list whose row heights are known.
 *
 *  `heights` is read reactively, so a caller derives it from its rows and the
 *  window follows. Attach the returned action to the scroll container. */
export function createVirtualWindow(options: {
  heights: () => readonly number[];
  threshold?: number;
  overscan?: number;
}) {
  const threshold = options.threshold ?? DEFAULT_THRESHOLD;

  let scroller: HTMLElement | undefined = $state();
  let scrollTop = $state(0);
  let viewport = $state(0);

  const heights = $derived(options.heights());
  const active = $derived(heights.length > threshold);
  const offsets = $derived(active ? offsetsOf(heights) : []);
  const slice = $derived(
    active
      ? windowFor(offsets, scrollTop, viewport, options.overscan)
      : { first: 0, last: heights.length, before: 0, after: 0 },
  );

  return {
    /** Svelte action for the scroll container. */
    attach(element: HTMLElement) {
      scroller = element;
      viewport = element.clientHeight;
      scrollTop = element.scrollTop;
      const observer = new ResizeObserver(() => {
        viewport = element.clientHeight;
      });
      observer.observe(element);
      const onscroll = () => {
        scrollTop = element.scrollTop;
      };
      element.addEventListener("scroll", onscroll, { passive: true });
      return {
        destroy() {
          observer.disconnect();
          element.removeEventListener("scroll", onscroll);
          scroller = undefined;
        },
      };
    },

    /** False while the list renders plainly, which keeps spacers out of the DOM. */
    get active() {
      return active;
    },
    get window() {
      return slice;
    },

    /** Brings a row into view whether or not it is currently rendered. Native
     *  scrollIntoView cannot: a row outside the window has no element. */
    scrollToIndex(index: number) {
      if (!scroller) return;
      if (!active) {
        scroller
          .querySelector<HTMLElement>(`[data-virtual-index="${index}"]`)
          ?.scrollIntoView({ block: "nearest" });
        return;
      }
      const top = offsets[index] ?? 0;
      const bottom = offsets[index + 1] ?? top;
      const view = scroller.scrollTop;
      if (top < view) scroller.scrollTop = top;
      else if (bottom > view + viewport) scroller.scrollTop = bottom - viewport;
    },
  };
}
