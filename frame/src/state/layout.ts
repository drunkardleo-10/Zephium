import { createSignal } from "solid-js";
import type { DividerView } from "../ipc/bindings";
import { commands, events } from "../ipc/bindings";

const [dividers, setDividers] = createSignal<DividerView[]>([]);

export { dividers };

let unlisten: (() => void) | null = null;

export async function init() {
  unlisten = await events.layoutChanged.listen((e) => setDividers(e.payload.dividers));
}

export function dispose() {
  unlisten?.();
  unlisten = null;
}

export const grab = (x: number, y: number) => void commands.dividerGrab(x, y);
export const drag = (x: number, y: number) => void commands.dividerDrag(x, y);
export const release = () => void commands.dividerRelease();
