const QUIET_MS = 250;

/**
 * WebKit composites the canvas's transformed viewport while anything inside
 * it moves (an arrival, a board settling, a helper walking), and keeps it
 * composited after the motion ends: a backing store the size of everything on
 * the canvas, tens to hundreds of megabytes, held at rest. One invisible
 * layout change once the canvas is still lets WebKit take the layer back.
 * Where WebKit already does, the change costs one small layout and nothing more.
 */
export function layerRelease(root: HTMLElement) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let gone = false;
  const watched = new WeakSet<Animation>();
  const release = () => {
    timer = undefined;
    const viewport = root.querySelector<HTMLElement>(".svelte-flow__viewport");
    if (gone || !viewport) return;
    const moving = viewport
      .getAnimations({ subtree: true })
      .filter((animation) => animation.playState === "running" || animation.pending);
    if (moving.length) {
      // Asked again as each one ends: never while something still moves, never on a timer.
      for (const animation of moving) {
        if (watched.has(animation)) continue;
        watched.add(animation);
        void animation.finished.then(moved, moved);
      }
      return;
    }
    const probe = document.createElement("div");
    probe.setAttribute("aria-hidden", "true");
    probe.style.position = "absolute";
    probe.style.inlineSize = "0";
    probe.style.blockSize = "0";
    viewport.append(probe);
    void viewport.offsetHeight;
    probe.remove();
  };
  /** Something on the canvas started or stopped moving: once all is still, the layer goes. */
  const moved = () => {
    if (gone) return;
    clearTimeout(timer);
    timer = setTimeout(release, QUIET_MS);
  };
  const events = ["transitionend", "transitioncancel", "animationend", "animationcancel"];
  for (const name of events) root.addEventListener(name, moved, true);
  return {
    moved,
    destroy() {
      gone = true;
      clearTimeout(timer);
      for (const name of events) root.removeEventListener(name, moved, true);
    },
  };
}
