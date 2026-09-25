/** A pointer drag gesture, modelled on the tab column's.
 *
 *  Not an HTML5 drag: that API cannot style what it carries, fires no move
 *  events over its own source, and behaves differently on every platform. This
 *  is the same pointer-capture gesture the sidebar already uses — a movement
 *  threshold so a click stays a click, capture so the pointer cannot escape the
 *  element, and a frame-throttled position so a drag costs one layout a frame.
 */
export type DragPosition = { x: number; y: number };

/** Pixels of movement before a press becomes a drag rather than a click. */
const THRESHOLD = 4;

export function createPointerDrag<T>(options: {
  /** Where the pointer let go. `target` is whatever sits under it. */
  ondrop: (item: T, target: Element | null, at: DragPosition) => void;
  threshold?: number;
}) {
  const threshold = options.threshold ?? THRESHOLD;

  let carrying = $state<T | null>(null);
  let at = $state<DragPosition>({ x: 0, y: 0 });
  let origin: DragPosition | null = null;
  let pending: T | null = null;
  let queued: DragPosition = { x: 0, y: 0 };
  let frame = 0;
  /** A drag ends with a click the row must not act on. */
  let consumed = false;

  function reset() {
    if (frame !== 0) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
    origin = null;
    pending = null;
    carrying = null;
  }

  function release(event: PointerEvent) {
    const target = event.currentTarget;
    if (target instanceof HTMLElement && target.hasPointerCapture(event.pointerId))
      target.releasePointerCapture(event.pointerId);
  }

  return {
    /** The item being carried, or null while nothing is. */
    get item() {
      return carrying;
    },
    get at() {
      return at;
    },
    /** True exactly once after a drag, so the click it ends with is ignored. */
    absorbClick() {
      if (!consumed) return false;
      consumed = false;
      return true;
    },
    begin(event: PointerEvent, item: T) {
      if (event.button !== 0) return;
      // A drag released away from its source ends without a click, so the
      // click it would have absorbed is not this press's.
      consumed = false;
      origin = { x: event.clientX, y: event.clientY };
      pending = item;
    },
    move(event: PointerEvent) {
      if (origin === null || pending === null) return;
      if (
        carrying === null &&
        Math.abs(event.clientX - origin.x) + Math.abs(event.clientY - origin.y) <= threshold
      )
        return;
      if (carrying === null) {
        const target = event.currentTarget;
        if (target instanceof HTMLElement) target.setPointerCapture(event.pointerId);
        carrying = pending;
      }
      queued = { x: event.clientX, y: event.clientY };
      if (frame !== 0) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        at = queued;
      });
    },
    end(event: PointerEvent) {
      const item = carrying;
      release(event);
      reset();
      if (item === null) return;
      consumed = true;
      // Read the drop target before anything re-renders under the pointer.
      options.ondrop(item, document.elementFromPoint(event.clientX, event.clientY), {
        x: event.clientX,
        y: event.clientY,
      });
    },
    cancel(event: PointerEvent) {
      release(event);
      reset();
    },
  };
}
