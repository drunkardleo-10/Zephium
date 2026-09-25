import { duration, easing } from "$shared/lib/motion";

/**
 * Reordering a list in place. The row being dragged follows the pointer and
 * the rows it passes slide aside to open its slot, all by transform, so the
 * list never lays itself out while a finger is down. Nothing is committed
 * until the drop; then the transforms are held until the reordered list
 * arrives and hands its rows to the list's own motion, which lands each one
 * from wherever it is drawn.
 */

const KEY = "data-motion-key";

type Slot = { element: HTMLElement; rect: DOMRect };

export class Reorder {
  #slots: Slot[];
  #from: number;
  #index: number;
  #startY: number;
  #step: number;
  #inside = false;

  private constructor(slots: Slot[], from: number, startY: number, step: number) {
    this.#slots = slots;
    this.#from = from;
    this.#index = from;
    this.#startY = startY;
    this.#step = step;
  }

  /** Starts a reorder of the direct row `key` in `list`, or declines when
   *  the row is not one the list can reorder by itself. */
  static begin(list: HTMLElement | undefined, key: string, pointerY: number): Reorder | null {
    if (!list) return null;
    const slots = [...list.querySelectorAll<HTMLElement>(`:scope > [${KEY}]`)]
      .filter((element) => element.getClientRects().length > 0)
      .map((element) => ({ element, rect: element.getBoundingClientRect() }));
    const from = slots.findIndex((slot) => slot.element.getAttribute(KEY) === key);
    if (from < 0 || slots.length < 2) return null;
    const next = slots[from + 1] ?? slots[from - 1]!;
    const step =
      from + 1 < slots.length
        ? next.rect.top - slots[from]!.rect.top
        : slots[from]!.rect.top - next.rect.top;
    const motion = `transform ${duration("base")}ms ${easing("emphasized")}`;
    // The dragged row's position answers the pointer directly; only its lift
    // eases in.
    const lift = ["scale", "box-shadow", "background-color"]
      .map((property) => `${property} ${duration("fast")}ms ${easing("out")}`)
      .join(", ");
    for (const [index, slot] of slots.entries()) {
      slot.element.style.transition = index === from ? lift : motion;
    }
    const dragged = slots[from]!.element;
    dragged.dataset.lifted = "";
    return new Reorder(slots, from, pointerY, Math.abs(step));
  }

  /** Follows the pointer. Returns whether it is over the list, where a drop
   *  reorders; elsewhere the list rests and the drop means something else. */
  update(x: number, y: number): boolean {
    const first = this.#slots[0]!.rect;
    const last = this.#slots.at(-1)!.rect;
    const inside =
      x >= first.left - 12 &&
      x <= first.right + 12 &&
      y >= first.top - this.#step / 2 &&
      y <= last.bottom + this.#step / 2;
    const dragged = this.#slots[this.#from]!;
    if (!inside) {
      if (this.#inside) this.#rest();
      this.#inside = false;
      return false;
    }
    this.#inside = true;
    dragged.element.dataset.lifted = "";

    const travel = Math.max(
      first.top - dragged.rect.top,
      Math.min(last.top - dragged.rect.top, y - this.#startY),
    );
    dragged.element.style.transform = `translateY(${travel}px)`;
    const centre = dragged.rect.top + dragged.rect.height / 2 + travel;
    let index = 0;
    for (const [at, slot] of this.#slots.entries()) {
      if (at === this.#from) continue;
      // A tie counts as passed: travel stops with the row's centre on the
      // last row's, and the end of the list must still be reachable.
      if (slot.rect.top + slot.rect.height / 2 <= centre) index += 1;
    }
    this.#index = index;
    for (const [at, slot] of this.#slots.entries()) {
      if (at === this.#from) continue;
      const shift =
        at > this.#from && at <= index
          ? -this.#step
          : at < this.#from && at >= index
            ? this.#step
            : 0;
      slot.element.style.transform = shift === 0 ? "" : `translateY(${shift}px)`;
    }
    return true;
  }

  /** Where the drop lands: whether anything moved, and the tab it goes before. */
  target(): { changed: boolean; before: string | null } {
    if (!this.#inside || this.#index === this.#from) return { changed: false, before: null };
    // The first tab at or after the slot: a folder has no tab of its own to
    // stand before, so the drop lands before the next row that does.
    const others = this.#slots.filter((_, at) => at !== this.#from).slice(this.#index);
    for (const { element } of others) {
      const tab = element.dataset.zephiumTabId
        ? element
        : element.querySelector<HTMLElement>("[data-zephium-tab-id]");
      if (tab?.dataset.zephiumTabId) return { changed: true, before: tab.dataset.zephiumTabId };
    }
    return { changed: true, before: null };
  }

  /** No reorder after all: everything slides back to where it was. */
  release() {
    const dragged = this.#slots[this.#from]!.element;
    dragged.style.transition = `transform ${duration("base")}ms ${easing("emphasized")}`;
    this.#rest();
    const done = () => this.clear();
    setTimeout(done, duration("base"));
  }

  /** The reordered list has arrived: drop the transforms without animating,
   *  after its motion has measured where every row was drawn. */
  clear() {
    for (const { element } of this.#slots) {
      element.style.transition = "none";
      element.style.transform = "";
      delete element.dataset.lifted;
    }
    requestAnimationFrame(() => {
      for (const { element } of this.#slots) element.style.transition = "";
    });
  }

  #rest() {
    for (const { element } of this.#slots) element.style.transform = "";
    delete this.#slots[this.#from]!.element.dataset.lifted;
  }
}
