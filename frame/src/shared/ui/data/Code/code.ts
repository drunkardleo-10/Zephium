export type CodeNoteView = { from: number; to: number; text: string };

export function noteOwners(notes: readonly CodeNoteView[], lines: number): (number | undefined)[] {
  const owners: (number | undefined)[] = Array.from({ length: lines });
  notes.forEach((note, index) => {
    const from = Math.max(1, Math.trunc(note.from));
    const to = Math.min(lines, Math.trunc(note.to));
    for (let line = from; line <= to; line += 1) owners[line - 1] ??= index;
  });
  return owners;
}
