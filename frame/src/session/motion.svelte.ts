let launch = $state(false);
let launchTimer: ReturnType<typeof setTimeout> | undefined;
let launched = false;
const reduce = () =>
  window.matchMedia("(prefers-reduced-motion: reduce)").matches ||
  document.documentElement.dataset.reduceMotion === "true";
export const launchActive = () => launch;
export function reveal() {
  if (launched) return;
  launched = true;
  if (reduce()) return;
  launch = true;
  launchTimer = setTimeout(() => {
    launch = false;
  }, 240);
}
export function dispose() {
  clearTimeout(launchTimer);
  launch = false;
}
