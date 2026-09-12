let phase = $state<"idle" | "exit" | "enter">("idle");
let launch = $state(false);
let sequence = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let launchTimer: ReturnType<typeof setTimeout> | undefined;
let launched = false;
const reduce = () =>
  window.matchMedia("(prefers-reduced-motion: reduce)").matches ||
  document.documentElement.dataset.reduceMotion === "true";
export const transitionPhase = () => phase;
export const launchActive = () => launch;
export function transition(commit: () => void) {
  const id = ++sequence;
  clearTimeout(timer);
  if (reduce()) {
    phase = "idle";
    commit();
    return;
  }
  phase = "exit";
  timer = setTimeout(() => {
    if (id !== sequence) return;
    commit();
    phase = "enter";
    timer = setTimeout(() => {
      if (id === sequence) phase = "idle";
    }, 140);
  }, 60);
}
export function reveal() {
  if (launched) return;
  launched = true;
  if (reduce()) return;
  launch = true;
  launchTimer = setTimeout(() => {
    launch = false;
  }, 240);
}
export function cancel() {
  sequence++;
  clearTimeout(timer);
  phase = "idle";
}
export function dispose() {
  cancel();
  clearTimeout(launchTimer);
  launch = false;
}
