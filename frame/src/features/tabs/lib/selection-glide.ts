import { captureFrom, playTo, type Origin } from "$shared/lib/plate-glide";

/** The current tab's plate, wherever the tab is drawn: a row, a tile, the rail. */
function plateOf(id: string | null): HTMLElement | null {
  if (!id) return null;
  const host = document.querySelector<HTMLElement>(`[data-zephium-tab-id="${CSS.escape(id)}"]`);
  if (!host) return null;
  return host.matches("[data-plate]") ? host : host.querySelector<HTMLElement>("[data-plate]");
}

/** Before the active tab changes: where its plate is drawn. */
export const capture = (id: string | null): Origin | null => captureFrom(plateOf(id));

/** After it changes: carry the plate to the tab that is now active. */
export const play = (origin: Origin | null, id: string | null) => playTo(origin, plateOf(id));
