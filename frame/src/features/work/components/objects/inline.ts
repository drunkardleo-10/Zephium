/** A run of inline text: plain, **strong** or `code`. Nothing else is read as markup. */
export type Run = { text: string; strong?: boolean; code?: boolean };

export function inline(text: string): Run[] {
  const runs: Run[] = [];
  const pattern = /(\*\*([^*]+)\*\*|`([^`]+)`)/gu;
  let at = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > at) runs.push({ text: text.slice(at, match.index) });
    runs.push(
      match[2] !== undefined ? { text: match[2], strong: true } : { text: match[3]!, code: true },
    );
    at = match.index + match[0].length;
  }
  if (at < text.length) runs.push({ text: text.slice(at) });
  return runs;
}
