import { duration, easing, reducedMotion } from "./motion";

/**
 * A selection's plate travels to the next selection instead of vanishing
 * from one item and appearing on another — the current tab, the current
 * settings page, any list where one thing is the current one. One element
 * moves, on its own layer, inside the list's `data-glide-host` so labels and
 * marks stay drawn above it; the items involved simply stop painting their
 * own plate while it is in flight.
 *
 * Items name the element that paints their plate with `data-plate`, and a
 * scrolling list names itself `data-glide-scroller`, so a plate never flies
 * out of, or into, an item scrolled out of sight.
 */

export type Origin = {
  rect: DOMRect;
  style: Pick<CSSStyleDeclaration, "backgroundColor" | "boxShadow" | "borderRadius">;
};

let flight: { plate: HTMLElement; target: HTMLElement } | null = null;

/** The element's resting box: an in-flight list move is a transform that is
 *  about to be undone, and the plate has to land where the row will be. */
function restingRect(element: HTMLElement): DOMRect {
  const rect = element.getBoundingClientRect();
  const row = element.closest<HTMLElement>("[data-motion-key]") ?? element;
  const { m41 = 0, m42 = 0 } = new DOMMatrixReadOnly(getComputedStyle(row).transform);
  return new DOMRect(rect.x - m41, rect.y - m42, rect.width, rect.height);
}

/** Inside the scroller that shows it: a plate never flies out of, or into, a
 *  row the column has scrolled out of sight. */
function inView(element: HTMLElement, rect: DOMRect): boolean {
  const scroller = element.closest<HTMLElement>("[data-glide-scroller]");
  if (!scroller) return true;
  const bounds = scroller.getBoundingClientRect();
  return rect.top >= bounds.top - 1 && rect.bottom <= bounds.bottom + 1;
}

function land() {
  if (!flight) return;
  const { plate, target } = flight;
  flight = null;
  // Hand the plate back without the row's own fade replaying underneath it.
  target.style.transition = "none";
  delete target.dataset.gliding;
  void target.offsetWidth;
  target.style.transition = "";
  plate.remove();
}

/** Before the change: where the current plate is drawn right now. */
export function captureFrom(element: HTMLElement | null): Origin | null {
  if (reducedMotion()) return null;
  if (flight) {
    // A plate already travelling is the one on screen; the next leg starts
    // from wherever it has got to.
    const { plate } = flight;
    const style = getComputedStyle(plate.firstElementChild ?? plate);
    const origin = {
      rect: plate.getBoundingClientRect(),
      style: {
        backgroundColor: style.backgroundColor,
        boxShadow: style.boxShadow,
        borderRadius: style.borderRadius,
      },
    };
    land();
    return origin;
  }
  if (!element) return null;
  const rect = restingRect(element);
  if (!inView(element, rect)) return null;
  const style = getComputedStyle(element);
  // The row being left must not fade its own plate out under the one leaving.
  element.style.transition = "none";
  requestAnimationFrame(() => (element.style.transition = ""));
  return {
    rect,
    style: {
      backgroundColor: style.backgroundColor,
      boxShadow: style.boxShadow,
      borderRadius: style.borderRadius,
    },
  };
}

/** After the change: carry the plate from where it was to the new current. */
export function playTo(origin: Origin | null, target: HTMLElement | null) {
  const host = target?.closest<HTMLElement>("[data-glide-host]");
  if (!origin || !target || !host) return;
  const to = restingRect(target);
  if (!inView(target, to)) return;
  const dx = origin.rect.x - to.x;
  const dy = origin.rect.y - to.y;
  const distance = Math.hypot(dx, dy);
  if (distance < 1) return;

  const bounds = host.getBoundingClientRect();
  const place = (rect: DOMRect, element: HTMLElement) =>
    Object.assign(element.style, {
      left: `${rect.x - bounds.x}px`,
      top: `${rect.y - bounds.y + host.scrollTop}px`,
      width: `${rect.width}px`,
      height: `${rect.height}px`,
    });

  // Only a plate can become a plate of the same shape. Between a row and a
  // tile the old one lets go where it is and the new one lights in place:
  // stretching one into the other would read as a distortion, not a move.
  const sameShape =
    Math.abs(origin.rect.width - to.width) < 2 && Math.abs(origin.rect.height - to.height) < 2;
  if (!sameShape) {
    const fading = document.createElement("div");
    fading.className = "selection-glide";
    fading.setAttribute("aria-hidden", "true");
    place(origin.rect, fading);
    Object.assign(fading.style, origin.style);
    host.prepend(fading);
    const out = fading.animate([{ opacity: 1 }, { opacity: 0 }], {
      duration: duration("fast"),
      easing: easing("exit"),
      fill: "forwards",
    });
    void out.finished.catch(() => undefined).then(() => fading.remove());
    return;
  }

  // The plate's resting look, not the first frame of the row's own fade
  // towards it; the transition is handed back when the plate lands.
  target.style.transition = "none";
  const final = getComputedStyle(target);
  const plate = document.createElement("div");
  plate.className = "selection-glide";
  plate.setAttribute("aria-hidden", "true");
  place(to, plate);
  const fill = document.createElement("span");
  fill.className = "selection-glide-fill";
  Object.assign(fill.style, {
    backgroundColor: final.backgroundColor,
    boxShadow: final.boxShadow,
    borderRadius: final.borderRadius,
  });
  plate.append(fill);
  host.prepend(plate);
  target.dataset.gliding = "";
  flight = { plate, target };

  // It answers the click at once and is there in about a quarter of a
  // second; a longer journey adds a little, never enough to lag the page.
  const time = duration("base") + 40 + Math.min(distance, 400) * 0.1;
  const travel = plate.animate(
    [{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }],
    { duration: time, easing: easing("snap") },
  );
  // Drawn out along its path while it moves and whole again as it lands,
  // the way something with a little give is. Only a journey of more than a
  // row earns it; a neighbour is a step, not a flight.
  if (distance > to.height * 1.5) {
    const give = Math.min(0.14, distance / 1600);
    const vertical = Math.abs(dy) >= Math.abs(dx);
    const drawn = vertical
      ? `scale(${1 - give / 3}, ${1 + give})`
      : `scale(${1 + give}, ${1 - give / 3})`;
    fill.animate(
      [{ transform: "none" }, { transform: drawn, offset: 0.35 }, { transform: "none" }],
      { duration: time * 0.85, easing: easing("in-out") },
    );
  }
  void travel.finished
    .catch(() => undefined)
    .then(() => {
      if (flight?.plate === plate) land();
    });
}
