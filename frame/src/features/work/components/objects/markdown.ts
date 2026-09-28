/** A draft's Markdown subset as blocks: paragraphs and bullet lists; inline marks stay in the text. */
export type Block = { kind: "paragraph"; lines: string[] } | { kind: "list"; items: string[] };

export function blocks(markdown: string): Block[] {
  const out: Block[] = [];
  for (const chunk of markdown.replace(/\r\n?/gu, "\n").split(/\n{2,}/u)) {
    const lines = chunk.split("\n").filter((line) => line.trim());
    if (!lines.length) continue;
    if (lines.every((line) => /^\s*[-*•]\s+/u.test(line)))
      out.push({ kind: "list", items: lines.map((line) => line.replace(/^\s*[-*•]\s+/u, "")) });
    else out.push({ kind: "paragraph", lines });
  }
  return out;
}

/** A note's Markdown as blocks: its headings too. */
export type NoteBlock = Block | { kind: "heading"; text: string };

const BULLET = /^\s*[-*•]\s+/u;
const run = (lines: string[]): Block =>
  lines.every((line) => BULLET.test(line))
    ? { kind: "list", items: lines.map((line) => line.replace(BULLET, "")) }
    : { kind: "paragraph", lines };

export function noteBlocks(markdown: string): NoteBlock[] {
  return blocks(markdown).flatMap((block): NoteBlock[] => {
    if (block.kind !== "paragraph") return [block];
    const out: NoteBlock[] = [];
    let lines: string[] = [];
    for (const line of block.lines) {
      const heading = /^#{1,6}\s+(.*)$/u.exec(line);
      if (!heading) {
        lines.push(line);
        continue;
      }
      if (lines.length) out.push(run(lines));
      lines = [];
      out.push({ kind: "heading", text: heading[1]! });
    }
    if (lines.length) out.push(run(lines));
    return out;
  });
}
