import { commands } from "$shared/ipc/bindings";
import { settle } from "$domain/operations";
let id = $state<string | null>(null);
let over = $state(false);
let failed = $state(false);
let pending = false;
let gesture = 0;
export const draggedId = () => id;
export const overEssentials = () => over;
export const moveFailed = () => failed;
export function begin(value: string) {
  gesture++;
  id = value;
  failed = false;
}
export function hover(x: number, y: number) {
  over = !!document.elementFromPoint(x, y)?.closest("[data-essentials-drop]");
}
export function end() {
  const active = id !== null;
  const generation = ++gesture;
  id = null;
  over = false;
  if (active)
    void commands.tabDragOver(null, null).catch(() => {
      if (generation === gesture) failed = true;
    });
}
export async function move(value: string, essential: boolean, before: string | null = null) {
  if (pending) return;
  pending = true;
  failed = false;
  try {
    const result = await settle(commands.tabsSetEssential(value, essential, before), 5000);
    if (result.outcome === "failed" || result.outcome === "rejected") throw new Error("settlement");
  } catch {
    failed = true;
  } finally {
    pending = false;
  }
}
