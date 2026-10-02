/** A bookmark the browser asked to show: Bookmark This Page names the one it
 *  added (or found), and the panel opens on it. Consumed once. */
let pending = $state<string | null>(null);

export const requested = () => pending;

export function request(id: string) {
  pending = id;
}

export function take(): string | null {
  const id = pending;
  pending = null;
  return id;
}
