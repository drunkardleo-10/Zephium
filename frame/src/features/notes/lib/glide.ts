import { duration, easing, reducedMotion } from "$shared/lib/motion";

const KEY = "data-glide";
/** Past this far a row did not move within view; it was reordered off it. */
const MAX_DISTANCE = 1200;

/** Where each keyed row of a list is drawn, measured before it changes. */
export function measure(list: HTMLElement | undefined): Map<string, number> | null {
  if (!list || reducedMotion()) return null;
  const rows = new Map<string, number>();
  for (const row of list.querySelectorAll<HTMLElement>(`:scope > [${KEY}]`))
    rows.set(row.getAttribute(KEY)!, row.getBoundingClientRect().top);
  return rows;
}

/** Slides rows that moved from where `measure` found them. A row that is new
 *  or gone simply appears or disappears. Transform only. */
export function glide(list: HTMLElement | undefined, before: Map<string, number> | null) {
  if (!list || !before) return;
  for (const row of list.querySelectorAll<HTMLElement>(`:scope > [${KEY}]`)) {
    const from = before.get(row.getAttribute(KEY)!);
    if (from === undefined) continue;
    const distance = from - row.getBoundingClientRect().top;
    if (distance === 0 || Math.abs(distance) > MAX_DISTANCE) continue;
    row.animate([{ transform: `translateY(${distance}px)` }, { transform: "none" }], {
      duration: duration("slow"),
      easing: easing("emphasized"),
    });
  }
}
