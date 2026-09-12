import type { SearchResult, SearchContext } from "$shared/ipc/bindings";
export function resultIdentity(result: SearchResult): string {
  const action = result.action;
  if (action.type === "ActivateTab") return `tab:${action.id}`;
  if (action.type === "OpenUrl") return `url:${action.url}`;
  return `command:${action.id}`;
}
export function sameSearch(expected: SearchContext | null, actual: SearchContext | null): boolean {
  return (
    !!expected &&
    !!actual &&
    expected.session_id === actual.session_id &&
    expected.request_id === actual.request_id &&
    expected.window_id === actual.window_id &&
    expected.profile_id === actual.profile_id &&
    expected.space_id === actual.space_id
  );
}
export function keepSelection(
  selected: string | null,
  results: readonly SearchResult[],
): string | null {
  return results.some((result) => resultIdentity(result) === selected)
    ? selected
    : results[0]
      ? resultIdentity(results[0])
      : null;
}
