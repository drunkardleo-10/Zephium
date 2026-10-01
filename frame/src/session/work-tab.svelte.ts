/**
 * A tab chosen in the rail while Work shows: Work opens it over its canvas
 * instead of leaving for Browse. Each choice is a new request, so choosing
 * the same tab again after closing it opens it again.
 */
let request = $state.raw<{ tab: string; sequence: number } | null>(null);
let sequence = 0;

export const tabRequest = () => request;

export function requestTab(tab: string) {
  request = { tab, sequence: ++sequence };
}
