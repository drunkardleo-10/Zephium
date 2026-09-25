export type CodeNoteView = { from: number; to: number; text: string };
/** Lines as Rust counts them: a final newline ends the last line, it does not start one. */
export const codeLines = (text: string) => text.replace(/\r?\n$/u, "").split(/\r?\n/u);
/** How many lines a card shows before "+n lines". */
export const CODE_CARD_LINES = 14;

export function noteOwners(notes: readonly CodeNoteView[], lines: number): (number | undefined)[] {
  const owners: (number | undefined)[] = Array.from({ length: lines });
  notes.forEach((note, index) => {
    const from = Math.max(1, Math.trunc(note.from));
    const to = Math.min(lines, Math.trunc(note.to));
    for (let line = from; line <= to; line += 1) owners[line - 1] ??= index;
  });
  return owners;
}
