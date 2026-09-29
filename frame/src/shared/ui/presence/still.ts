/**
 * One watch for everything that moves on its own: it is still while it is off
 * screen, while the window is hidden, and while motion is reduced. A single
 * observer and two listeners serve every character and indicator.
 */

type Watch = { visible: boolean; notify: (still: boolean) => void };

const watched = new Map<Element, Watch>();
let observer: IntersectionObserver | undefined;
let reduced = false;
let listening = false;

const hidden = () => document.visibilityState === "hidden";

function reducedNow(): boolean {
  return (
    window.matchMedia("(prefers-reduced-motion: reduce)").matches ||
    document.documentElement.dataset.reduceMotion === "true"
  );
}

function tell(watch: Watch) {
  watch.notify(!watch.visible || hidden() || reduced);
}

function refresh() {
  reduced = reducedNow();
  for (const watch of watched.values()) tell(watch);
}

let attributes: MutationObserver | undefined;
let media: MediaQueryList | undefined;

function listen() {
  if (listening) return;
  listening = true;
  reduced = reducedNow();
  document.addEventListener("visibilitychange", refresh);
  media = window.matchMedia("(prefers-reduced-motion: reduce)");
  media.addEventListener("change", refresh);
  attributes = new MutationObserver(refresh);
  attributes.observe(document.documentElement, { attributeFilter: ["data-reduce-motion"] });
  observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      const watch = watched.get(entry.target);
      if (!watch || watch.visible === entry.isIntersecting) continue;
      watch.visible = entry.isIntersecting;
      tell(watch);
    }
  });
}

function unlisten() {
  if (!listening || watched.size) return;
  listening = false;
  document.removeEventListener("visibilitychange", refresh);
  media?.removeEventListener("change", refresh);
  attributes?.disconnect();
  observer?.disconnect();
  observer = undefined;
}

/** Calls `notify` now and whenever the element should stop or may move again. */
export function watchStill(element: Element, notify: (still: boolean) => void): () => void {
  listen();
  const watch: Watch = { visible: true, notify };
  watched.set(element, watch);
  observer!.observe(element);
  tell(watch);
  return () => {
    observer?.unobserve(element);
    watched.delete(element);
    unlisten();
  };
}
