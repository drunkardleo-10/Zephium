/** Keep the management dialog in the chrome document, outside the utility
 * popover's inert, clipped and transformed subtree. */
export function portal(node: HTMLElement, enabled = true) {
  if (enabled) document.body.append(node);
  return {
    destroy() {
      node.remove();
    },
  };
}
