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
