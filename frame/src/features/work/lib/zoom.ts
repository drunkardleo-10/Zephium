import type { Detail } from "./board/types";

/**
 * How much the canvas shows at a zoom: everything from 75%, the survey from
 * 40%, marks below. A level holds 3% past its edge on the way out, so a pinch
 * resting on a threshold never flickers between two.
 */
const FULL = 0.75;
const OVERVIEW = 0.4;
const HOLD = 0.03;

export function detailAt(zoom: number, current: Detail = "full"): Detail {
  const full = current === "full" ? FULL - HOLD : FULL;
  const overview =
    current === "tile" ? OVERVIEW + HOLD : current === "overview" ? OVERVIEW - HOLD : OVERVIEW;
  if (zoom >= full) return "full";
  if (zoom >= overview) return "overview";
  return "tile";
}
