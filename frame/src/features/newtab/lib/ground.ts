/** The page's ground: solid, with the window's frame reaching down into it
 *  through a notch in its top edge, and openings cut through it wherever the
 *  window itself should show. */
export type Notch = {
  /** Wanted width; narrowed to fit, never below what the corners need. */
  width: number;
  height: number;
  /** The rounding of the notch's two corners inside the page. */
  radius: number;
  /** Where the notch meets the top edge, the ground on either side turns up
   *  into it on this radius, so the notch drops out of the frame rather than
   *  being punched through the page. */
  flare: number;
};

/** A notch's width once fitted to a page of this width. */
export function notchWidth(width: number, notch: Notch): number {
  return Math.max(0, Math.min(notch.width, width - (2 * notch.flare + 16)));
}

const n = (value: number) => Math.round(value * 100) / 100;

/** The notch traced left to right, from its left foot on the top edge to
 *  its right one, or null when the page has no room for it. */
function contour(
  width: number,
  height: number,
  notch: Notch,
): { foot: number; path: string } | null {
  const span = notchWidth(width, notch);
  const depth = Math.min(notch.height, height - notch.flare);
  const { radius: r, flare: f } = notch;
  if (span < 2 * r || depth < r + f) return null;
  const left = n((width - span) / 2);
  const right = n((width + span) / 2);
  return {
    foot: n(left - f),
    path: [
      `A${f} ${f} 0 0 1 ${left} ${f}`,
      `V${n(depth - r)}`,
      `A${r} ${r} 0 0 0 ${n(left + r)} ${n(depth)}`,
      `H${n(right - r)}`,
      `A${r} ${r} 0 0 0 ${right} ${n(depth - r)}`,
      `V${f}`,
      `A${f} ${f} 0 0 1 ${n(right + f)} 0`,
    ].join(""),
  };
}

/** A closed rectangle with rounded corners, as one subpath. */
export function roundedRect(x: number, y: number, w: number, h: number, r: number): string {
  const k = Math.max(0, Math.min(r, w / 2, h / 2));
  return [
    `M${n(x + k)} ${n(y)}`,
    `H${n(x + w - k)}`,
    `A${k} ${k} 0 0 1 ${n(x + w)} ${n(y + k)}`,
    `V${n(y + h - k)}`,
    `A${k} ${k} 0 0 1 ${n(x + w - k)} ${n(y + h)}`,
    `H${n(x + k)}`,
    `A${k} ${k} 0 0 1 ${n(x)} ${n(y + h - k)}`,
    `V${n(y + k)}`,
    `A${k} ${k} 0 0 1 ${n(x + k)} ${n(y)}`,
    "Z",
  ].join("");
}

/** The ground's outline as an SVG path to fill even-odd: the page less the
 *  notch in its top edge, less every opening given. Nothing before the page
 *  has a size; a notch it has no room for is left out. */
export function groundPath(
  width: number,
  height: number,
  notch: Notch,
  openings: readonly string[] = [],
): string {
  if (width <= 0 || height <= 0) return "";
  const cut = contour(width, height, notch);
  const top = cut ? `H${cut.foot}${cut.path}` : "";
  return `M0 0${top}H${width}V${height}H0Z${openings.join("")}`;
}

/** The notch alone as a closed shape, shut along the top edge, for drawing
 *  its lip. Empty when the page has no room for it. */
export function notchShape(width: number, height: number, notch: Notch): string {
  if (width <= 0 || height <= 0) return "";
  const cut = contour(width, height, notch);
  return cut ? `M${cut.foot} 0${cut.path}Z` : "";
}
