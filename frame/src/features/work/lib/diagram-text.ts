/** A part's words as the card sets them: the name at 13 px semibold, its note at 12 px. */
export const PART_TEXT = {
  name: { size: 13, weight: 600, line: 17 },
  note: { size: 12, weight: 400, line: 15 },
} as const;

let context: CanvasRenderingContext2D | null | undefined;
const widths = new Map<string, number>();

/** A run of text's width in the interface face, measured once; estimated off the page. */
function measure(text: string, size: number, weight: number): number {
  const key = `${size}|${weight}|${text}`;
  const known = widths.get(key);
  if (known !== undefined) return known;
  if (context === undefined)
    context =
      typeof document === "undefined" ? null : document.createElement("canvas").getContext("2d");
  let width = text.length * size * (weight >= 600 ? 0.58 : 0.54);
  if (context) {
    const face = getComputedStyle(document.documentElement).getPropertyValue("--font-sans").trim();
    context.font = `${weight} ${size}px ${face || "system-ui"}`;
    width = context.measureText(text).width;
  }
  if (widths.size > 4000) widths.clear();
  widths.set(key, width);
  return width;
}

/** The lines words take at a measure, kept whole; Infinity when one word is wider than it. */
export function lineCount(
  text: string,
  room: number,
  style: { size: number; weight: number },
): number {
  const space = measure(" ", style.size, style.weight);
  let lines = 1;
  let used = 0;
  for (const word of text.split(/\s+/u).filter(Boolean)) {
    const width = measure(word, style.size, style.weight);
    if (width > room) return Infinity;
    const next = used ? used + space + width : width;
    if (next <= room) used = next;
    else {
      lines += 1;
      used = width;
    }
  }
  return lines;
}
