/**
 * The engine's own menu (Reload, Inspect Element) is about a web page, which
 * the browser's interface is not. Text fields keep theirs for Cut, Copy and
 * Paste; in development Option or Alt brings the engine's menu back.
 */
export function installChromeMenu(onmenu?: (event: MouseEvent) => void): () => void {
  const handle = (event: MouseEvent) => {
    if (event.defaultPrevented) return;
    if (import.meta.env.DEV && event.altKey) return;
    const target = event.target;
    if (
      target instanceof Element &&
      target.closest('input, textarea, [contenteditable]:not([contenteditable="false"])')
    )
      return;
    event.preventDefault();
    onmenu?.(event);
  };
  window.addEventListener("contextmenu", handle);
  return () => window.removeEventListener("contextmenu", handle);
}
