/** Moves a floating layer to the document's root. Inside the notes area it
 *  would be placed against the nearest container that contains its layout
 *  (a sized container, a moving view) rather than against the window its
 *  coordinates are measured in. */
export function portal(node: HTMLElement) {
  document.body.append(node);
  return {
    destroy() {
      node.remove();
    },
  };
}
