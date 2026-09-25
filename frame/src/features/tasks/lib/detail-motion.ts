import { duration, easing, reducedMotion } from "$shared/lib/motion";

/**
 * The page opening a task beside its list, and closing it again. The grid
 * changes in one step — the list lays itself out once at its new width —
 * and everything seen to move does so by transform: the list glides to
 * where it now sits, the task arrives from the side it lives on, and on
 * closing it leaves the same way instead of disappearing.
 */

export type PaneSnapshot = {
  column: DOMRect | null;
  detail: HTMLElement | null;
  detailRect: DOMRect | null;
};

const COLUMN = ".page-column";
const DETAIL = ":scope > .page-detail";

export function capture(stage: HTMLElement | undefined): PaneSnapshot | null {
  if (!stage || reducedMotion()) return null;
  const detail = stage.querySelector<HTMLElement>(DETAIL);
  return {
    column: stage.querySelector(COLUMN)?.getBoundingClientRect() ?? null,
    detail,
    detailRect: detail?.getBoundingClientRect() ?? null,
  };
}

export function play(stage: HTMLElement | undefined, snapshot: PaneSnapshot | null) {
  if (!stage || !snapshot) return;
  const time = duration("page");
  const column = stage.querySelector<HTMLElement>(COLUMN);
  const now = column?.getBoundingClientRect();
  if (column && now && snapshot.column) {
    const dx = snapshot.column.left - now.left;
    if (Math.abs(dx) > 1) {
      column.animate([{ transform: `translateX(${dx}px)` }, { transform: "none" }], {
        duration: time,
        easing: easing("emphasized"),
      });
    }
  }

  const detail = stage.querySelector<HTMLElement>(DETAIL);
  if (detail && detail !== snapshot.detail) {
    detail.animate(
      [
        { opacity: 0, transform: "translateX(28px)" },
        { opacity: 1, transform: "none" },
      ],
      { duration: time, easing: easing("emphasized"), fill: "backwards" },
    );
  }

  // Closed: the pane goes back the way it came, as an unreachable stand-in
  // at the place it last occupied.
  const left = snapshot.detail;
  const rect = snapshot.detailRect;
  if (left && rect && !left.isConnected) {
    for (const node of [left, ...left.querySelectorAll<HTMLElement>("[id]")]) {
      node.removeAttribute("id");
    }
    left.setAttribute("aria-hidden", "true");
    left.inert = true;
    const bounds = stage.getBoundingClientRect();
    Object.assign(left.style, {
      position: "absolute",
      left: `${rect.left - bounds.left}px`,
      top: `${rect.top - bounds.top}px`,
      width: `${rect.width}px`,
      height: `${rect.height}px`,
      pointerEvents: "none",
    });
    stage.append(left);
    const out = left.animate(
      [
        { opacity: 1, transform: "none" },
        { opacity: 0, transform: "translateX(28px)" },
      ],
      { duration: duration("base"), easing: easing("exit"), fill: "forwards" },
    );
    void out.finished.catch(() => undefined).then(() => left.remove());
  }
}
