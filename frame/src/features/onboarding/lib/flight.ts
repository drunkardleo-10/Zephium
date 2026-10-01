/** A point on the quadratic curve from `a` to `b` bowed through `c`. */
function along(a: number, c: number, b: number, t: number) {
  return (1 - t) * (1 - t) * a + 2 * (1 - t) * t * c + t * t * b;
}

const SAMPLES = 16;

/**
 * A mark thrown from where it was chosen to where it now lives. It travels a
 * shallow arc rather than a straight line, the way a thing that is picked up
 * and set down moves, and shrinks to the size it lands at. The curve is
 * sampled into keyframes so one easing governs the whole path.
 */
export function throwMark(
  host: HTMLElement,
  source: string,
  from: DOMRect,
  to: DOMRect,
): Promise<void> {
  const image = document.createElement("img");
  image.src = source;
  image.alt = "";
  image.className = "flight";
  const size = from.width;
  Object.assign(image.style, {
    left: `${from.left}px`,
    top: `${from.top}px`,
    width: `${size}px`,
    height: `${size}px`,
  });
  host.append(image);

  const start = { x: from.left + from.width / 2, y: from.top + from.height / 2 };
  const end = { x: to.left + to.width / 2, y: to.top + to.height / 2 };
  // The arc rises by a share of the distance, so a short hop barely lifts
  // and a long throw clears what it passes over.
  const lift = Math.min(160, Math.hypot(end.x - start.x, end.y - start.y) * 0.28);
  const bend = { x: (start.x + end.x) / 2, y: Math.min(start.y, end.y) - lift };
  const landing = Math.min(to.width, to.height) / size;

  const frames: Keyframe[] = [];
  for (let i = 0; i <= SAMPLES; i++) {
    const t = i / SAMPLES;
    const x = along(start.x, bend.x, end.x, t) - start.x;
    const y = along(start.y, bend.y, end.y, t) - start.y;
    // Grows a touch as it lifts, then settles to its landing size.
    const scale = 1 + Math.sin(Math.PI * t) * 0.12 + (landing - 1) * t * t;
    frames.push({
      offset: t,
      transform: `translate(${x}px, ${y}px) scale(${scale})`,
      opacity: t < 0.86 ? 1 : 1 - (t - 0.86) / 0.14,
    });
  }
  const flight = image.animate(frames, {
    duration: 640,
    easing: "cubic-bezier(0.45, 0, 0.2, 1)",
    fill: "forwards",
  });
  return flight.finished.then(
    () => image.remove(),
    () => image.remove(),
  );
}
