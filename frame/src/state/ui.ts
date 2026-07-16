import { createSignal } from "solid-js";
import { events } from "../ipc/native-events";

const [command, setCommand] = createSignal<{ id: string; seq: number }>(
  { id: "", seq: 0 },
  { equals: false },
);

export const uiCommand = command;

let seq = 0;
let unlisten: (() => void) | null = null;

export async function init() {
  unlisten = await events.uiCommand.listen((e) => {
    seq += 1;
    setCommand({ id: e.payload, seq });
  });
}

export function dispose() {
  unlisten?.();
  unlisten = null;
}
