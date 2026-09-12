import type { ResourceSummary } from "$shared/ipc/bindings";
export function mergePage(
  previous: readonly ResourceSummary[],
  next: readonly ResourceSummary[],
): ResourceSummary[] {
  return [...new Map([...previous, ...next].map((row) => [row.id, row])).values()].slice(0, 1000);
}
export function newerRevision(candidate: string, current: string): boolean {
  const valid = (value: string) => /^[1-9][0-9]{0,18}$/u.test(value);
  return valid(candidate) && valid(current) && BigInt(candidate) > BigInt(current);
}

export function noteReferences(document: import("$shared/ipc/bindings").NoteDocument): string[] {
  const ids = new Set<string>();
  const pending = [document.document];
  while (pending.length) {
    const node = pending.pop()!;
    if (node.type === "noteReference" && node.attrs?.resource) ids.add(node.attrs.resource);
    pending.push(...(node.content ?? []));
  }
  return [...ids].sort().slice(0, 64);
}
