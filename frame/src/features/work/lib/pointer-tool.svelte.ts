import type { PointerTool } from "./selection";

/** Select or hand, shared by the canvas and the bar: remembered for this app session only. */
let tool = $state<PointerTool>("hand");

export const pointerTool = () => tool;

export function setPointerTool(next: PointerTool) {
  tool = next;
}
