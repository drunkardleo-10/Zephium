import { createSignal } from "solid-js";
import type { DividerView } from "../ipc/bindings";
import { commands } from "../ipc/bindings";
import { events } from "../ipc/native-events";

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
export const release = (x: number | null, y: number | null) => void commands.dividerRelease(x, y);
