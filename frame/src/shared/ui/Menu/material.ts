/** Carry the trigger's surface policy across a DOM portal. No global menu override. */
export function menuMaterialFor(trigger: HTMLElement | null | undefined): "opaque" | undefined {
  return trigger?.closest("[data-menu-material]")?.getAttribute("data-menu-material") === "opaque"
    ? "opaque"
    : undefined;
}
