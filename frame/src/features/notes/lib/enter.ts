import { duration, easing, reducedMotion } from "$shared/lib/motion";

/** Moves a view in from the side it is navigated to: forward from the
 *  trailing edge, back from the leading one. Transform and opacity only. */
export function enter(node: HTMLElement, direction: "forward" | "back" | null) {
  if (!direction || reducedMotion()) return;
  const rtl = getComputedStyle(node).direction === "rtl";
  const offset = (direction === "forward" ? 18 : -18) * (rtl ? -1 : 1);
  node.animate(
    [
      { transform: `translateX(${offset}px)`, opacity: 0 },
      { transform: "none", opacity: 1 },
    ],
    { duration: duration("slow"), easing: easing("emphasized") },
  );
}
