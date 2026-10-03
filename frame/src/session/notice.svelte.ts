/** One brief confirmation at the foot of the sidebar, such as a copied link.
 *  A newer notice replaces the one showing; nothing queues. */
type Notice = { id: number; text: string };

let current = $state.raw<Notice | null>(null);
let next = 0;

export const notice = () => current;

export function show(text: string) {
  next += 1;
  current = { id: next, text };
}

/** Clears only the notice that asked, so a newer one is never cut short. */
export function dismiss(id: number) {
  if (current?.id === id) current = null;
}
