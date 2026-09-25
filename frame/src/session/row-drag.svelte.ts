import { tabs } from "$domain/tabs";
import * as tabDrag from "./tab-drag.svelte";
import { Reorder } from "./reorder";

/** Where a drag was let go, for the list to decide what that means. */
type Drop = {
  id: string;
  x: number;
  y: number;
  /** The element under the pointer, resolved before anything moved. */
  over: Element | null;
  /** The tab under the pointer, which a placement goes before. */
  before: string | null;
};

type Options = {
  /** The list whose direct rows reorder in place. */
  list: () => HTMLElement | undefined;
  /** Whether this list reorders in place, and whether its rows are kept. */
  reorder: () => { essential: boolean } | null;
  /** A drop anywhere but back into the list's own order. */
  drop: (drop: Drop) => void;
};

/**
 * One drag gesture for every list of tabs: the sidebar's rows, the rail, the
 * kept sites. A press becomes a drag only past a small movement, so a click
 * stays a click; the pointer is captured, followed once per frame, and over
 * the list's own rows the row itself travels (see reorder.ts) while anywhere
 * else a stand-in follows it. Letting go inside the list reorders it; letting
 * go anywhere else is the list's to interpret.
 */
export class RowDrag {
  /** The stand-in, while the pointer is away from the list's own rows. */
  ghost = $state<{ id: string; x: number; y: number } | null>(null);

  #options: Options;
  #down: { x: number; y: number } | null = null;
  #id = "";
  #dragging = false;
  #pointer = { x: 0, y: 0 };
  #frame = 0;
  #swallow = false;
  #reorder: Reorder | null = null;
  // A drop already committed, held where it was drawn until the reordered
  // list arrives.
  #landing: Reorder | null = null;
  #essential = false;

  constructor(options: Options) {
    this.#options = options;
  }

  down(event: PointerEvent, id: string) {
    if (event.button !== 0) return;
    this.#down = { x: event.clientX, y: event.clientY };
    this.#id = id;
    this.#dragging = false;
  }

  move(event: PointerEvent) {
    const start = this.#down;
    if (start === null) return;
    if (
      !this.#dragging &&
      Math.abs(event.clientX - start.x) + Math.abs(event.clientY - start.y) > 4
    ) {
      const target = event.currentTarget;
      if (target instanceof HTMLElement) target.setPointerCapture(event.pointerId);
      this.#dragging = true;
      tabDrag.begin(this.#id);
      document.body.style.cursor = "grabbing";
      const reorder = this.#options.reorder();
      this.#essential = reorder?.essential ?? false;
      this.#reorder = reorder
        ? Reorder.begin(this.#options.list(), `tab:${this.#id}`, event.clientY)
        : null;
    }
    if (!this.#dragging) return;

    this.#pointer = { x: event.clientX, y: event.clientY };
    if (this.#frame !== 0) return;
    this.#frame = requestAnimationFrame(() => {
      this.#frame = 0;
      const { x, y } = this.#pointer;
      const inPlace = this.#reorder?.update(x, y) ?? false;
      this.ghost = inPlace ? null : { id: this.#id, x, y };
      tabDrag.hover(x, y);
      tabs.dragOver(x, y);
    });
  }

  up(event: PointerEvent) {
    const over = document.elementFromPoint(event.clientX, event.clientY);
    const before =
      over?.closest<HTMLElement>("[data-zephium-tab-id]")?.dataset.zephiumTabId ?? null;
    const dragged = this.#dragging;
    const id = this.#id;
    const reorder = this.#reorder;
    const essential = this.#essential;
    if (reorder && this.#frame !== 0) reorder.update(event.clientX, event.clientY);
    this.#release(event);
    this.#reset();
    if (!dragged) return;
    this.#swallow = true;

    const placed = reorder?.target();
    if (reorder && placed?.changed) {
      this.#landing?.clear();
      this.#landing = reorder;
      void tabDrag.move(id, essential, placed.before).then(() => {
        if (this.#landing !== reorder) return;
        // Refused: slide home at once. Accepted: the reordered list is on
        // its way and lands the rows itself; if it never comes, slide home.
        const home = () => {
          if (this.#landing !== reorder) return;
          this.#landing = null;
          reorder.release();
        };
        if (tabDrag.moveFailed()) home();
        else setTimeout(home, 600);
      });
      return;
    }
    reorder?.release();
    this.#options.drop({
      id,
      x: event.clientX,
      y: event.clientY,
      over,
      before: before === id ? null : before,
    });
  }

  cancel(event: PointerEvent) {
    this.#release(event);
    this.#reorder?.release();
    this.#reset();
  }

  /** Abandons any drag in progress, as a context menu opening does. */
  abandon() {
    this.#reorder?.release();
    this.#reset();
  }

  /** Whether a click is the end of a drag, and so not a click at all. */
  swallowClick(): boolean {
    const swallow = this.#swallow;
    this.#swallow = false;
    return swallow;
  }

  /** The list has changed shape: a held drop has arrived. Call after the
   *  list's own motion has measured where every row was drawn. */
  landed() {
    this.#landing?.clear();
    this.#landing = null;
  }

  #release(event: PointerEvent) {
    const target = event.currentTarget;
    if (target instanceof HTMLElement && target.hasPointerCapture(event.pointerId)) {
      target.releasePointerCapture(event.pointerId);
    }
  }

  #reset() {
    if (this.#frame !== 0) {
      cancelAnimationFrame(this.#frame);
      this.#frame = 0;
    }
    this.#down = null;
    this.#id = "";
    this.#dragging = false;
    this.#reorder = null;
    this.ghost = null;
    document.body.style.cursor = "";
    tabDrag.end();
  }
}
