import type { IconSvgElement } from "@hugeicons/svelte";
import type { TaskPriority } from "$domain/resources";

/** Linear's signal: three rising bars, as many lit as the priority is high.
 *  Monochrome on purpose — the browser column beside it already carries every
 *  site's colours, and priority should read as weight, not as another badge. */
function bars(lit: number): IconSvgElement {
  return [0, 1, 2].map((index) => [
    "rect",
    {
      x: String(3.5 + index * 6.5),
      y: String(14 - index * 4.5),
      width: "4",
      height: String(6 + index * 4.5),
      rx: "1.25",
      fill: "currentColor",
      stroke: "none",
      opacity: index < lit ? "1" : "0.28",
      key: String(index),
    },
  ]) as unknown as IconSvgElement;
}

export const PRIORITY_ICON: Record<TaskPriority, IconSvgElement> = {
  none: bars(0),
  low: bars(1),
  medium: bars(2),
  high: bars(3),
};
