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
