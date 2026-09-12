import type { Action } from "svelte/action";
const positions = new Map<string, number>();
export const rememberScroll: Action<HTMLElement, string> = (node, initial) => {
  let key = initial;
  let live = true;
  const restore = () =>
    queueMicrotask(() => {
      if (live) node.scrollTop = positions.get(key) ?? 0;
    });
  const save = () => {
    positions.set(key, node.scrollTop);
    if (positions.size > 32) {
      const first = positions.keys().next().value;
      if (first) positions.delete(first);
    }
  };
  restore();
  return {
    update(next) {
      if (next === key) return;
      save();
      key = next;
      restore();
    },
    destroy() {
      save();
      live = false;
    },
  };
};
