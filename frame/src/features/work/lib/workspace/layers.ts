const QUIET_MS = 250;
/** A person at the canvas keeps its layer this long after their last move. */
const HELD_MS = 12_000;

/**
 * WebKit composites the canvas's transformed viewport while anything inside
 * it moves (an arrival, a board settling, a helper walking), and keeps it
 * composited after the motion ends: a backing store the size of everything on
 * the canvas, tens to hundreds of megabytes, held at rest. One invisible
 * layout change once the canvas is still lets WebKit take the layer back.
 * Where WebKit already does, the change costs one small layout and nothing more.
 *
 * A person at the canvas is the exception: while their pointer or a pan or
 * pinch is on it, the viewport is held as its own layer, so moving it is the
 * compositor sliding and scaling a picture, never a repaint of everything on
 * it. It is let go after a while of rest, or at once when the window hides.
 */
export function layerRelease(root: HTMLElement) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let held: ReturnType<typeof setTimeout> | undefined;
  let holding = false;
  let gone = false;
  const viewportOf = () => root.querySelector<HTMLElement>(".svelte-flow__viewport");
  const watched = new WeakSet<Animation>();
  const release = () => {
    timer = undefined;
    const viewport = viewportOf();
    if (gone || holding || !viewport) return;
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
  const letGo = () => {
    clearTimeout(held);
    held = undefined;
    if (!holding) return;
    holding = false;
    const viewport = viewportOf();
    if (viewport) viewport.style.willChange = "";
    moved();
  };
  /** The person is at the canvas: its layer is held, and the hold is renewed. */
  const engage = () => {
    if (gone) return;
    if (!holding) {
      const viewport = viewportOf();
      if (!viewport) return;
      holding = true;
      viewport.style.willChange = "transform";
    }
    clearTimeout(held);
    held = setTimeout(letGo, HELD_MS);
  };
  const hidden = () => {
    if (document.visibilityState === "hidden") letGo();
  };
  const events = ["transitionend", "transitioncancel", "animationend", "animationcancel"];
  for (const name of events) root.addEventListener(name, moved, true);
  const presence = ["pointermove", "wheel", "pointerdown"] as const;
  for (const name of presence) root.addEventListener(name, engage, { passive: true });
  document.addEventListener("visibilitychange", hidden);
  return {
    moved,
    engage,
    destroy() {
      gone = true;
      clearTimeout(timer);
      clearTimeout(held);
      for (const name of events) root.removeEventListener(name, moved, true);
      for (const name of presence) root.removeEventListener(name, engage);
      document.removeEventListener("visibilitychange", hidden);
    },
  };
}
