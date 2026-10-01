/**
 * Calls back when an element comes within a screen of the viewport and when it
 * leaves again, so heavy content (a chart, a player) exists only near the view.
 */
export function watchNear(element: Element, onchange: (near: boolean) => void): () => void {
  const watch = new IntersectionObserver(
    (entries) => {
      const entry = entries.at(-1);
      if (entry) onchange(entry.isIntersecting);
    },
    { rootMargin: "100% 100%" },
  );
  watch.observe(element);
  return () => watch.disconnect();
}
