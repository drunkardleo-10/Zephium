/** A note's title and preview as the list shows them while it is being
 *  edited, before the saved file's own description comes back. It runs on
 *  every keystroke, so it reads only as many lines as a preview needs. */
const PREVIEW_CHARS = 180;
const FRONT_MATTER = /^---\n[\s\S]*?\n(?:---|\.\.\.)[ \t]*(?:\n|$)/u;

function plain(line: string): string {
  return line
    .replace(
      /!?\[\[([^\]|]*)\|?([^\]]*)\]\]/gu,
      (_all, target: string, alias: string) => alias || target,
    )
    .replace(/!?\[([^\]]*)\]\([^)]*\)/gu, "$1")
    .replace(/^\s{0,3}(?:[-+*]|\d{1,9}[.)])\s+(?:\[[ xX]\]\s+)?/u, "")
    .replace(/^\s{0,3}>\s?/u, "")
    .replace(/[*_~`]+/gu, "")
    .replace(/\\(.)/gu, "$1")
    .trim();
}

function* lines(text: string, from: number): Generator<string> {
  let start = from;
  while (start <= text.length) {
    const end = text.indexOf("\n", start);
    yield text.slice(start, end === -1 ? text.length : end);
    if (end === -1) return;
    start = end + 1;
  }
}

export function outline(markdown: string): { heading: string | null; preview: string } {
  const front = markdown.startsWith("---\n") ? FRONT_MATTER.exec(markdown) : null;
  let heading: string | null = null;
  let seen = false;
  let preview = "";
  for (const line of lines(markdown, front ? front[0].length : 0)) {
    if (!seen) {
      if (line.trim() === "") continue;
      seen = true;
      const title = /^\s{0,3}#{1,6}\s+(.*?)\s*#*\s*$/u.exec(line);
      if (title) {
        heading = plain(title[1]!) || null;
        continue;
      }
    }
    if (/^\s*(?:```|~~~|---\s*$)/u.test(line)) continue;
    // A single enormous line needs no more than a preview's worth of cleaning.
    const text = plain(line.slice(0, 4 * PREVIEW_CHARS).replace(/^\s{0,3}#{1,6}\s+/u, ""));
    if (text) preview += (preview ? " " : "") + text;
    if (preview.length > PREVIEW_CHARS) break;
  }
  const chars = [...preview];
  return {
    heading,
    preview:
      chars.length > PREVIEW_CHARS
        ? `${chars.slice(0, PREVIEW_CHARS).join("").trimEnd()}…`
        : preview,
  };
}

/** Whether a note holds nothing worth keeping. */
export function blank(markdown: string): boolean {
  return markdown.replace(/^#{1,6}\s*$/gmu, "").trim() === "";
}
