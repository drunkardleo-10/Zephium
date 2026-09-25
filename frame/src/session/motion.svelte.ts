import { reducedMotion } from "$shared/lib/motion";

/**
 * The launch cascade. It is armed before the first frame is drawn, holding
 * every row at the start of its entrance, and runs when native reveals the
 * window — so the first thing anyone sees is the list arriving, never a list
 * that is already there and then vanishes to arrive again.
 */
type Launch = "armed" | "running" | "idle";

let launch = $state<Launch>(reducedMotion() ? "idle" : "armed");
let launchTimer: ReturnType<typeof setTimeout> | undefined;
// Rows must never stay held if the reveal is lost: past this, they show.
const fallback: ReturnType<typeof setTimeout> | undefined =
  launch === "armed" ? setTimeout(() => (launch = "idle"), 2500) : undefined;
let launched = false;

export const launchState = () => launch;

export function reveal() {
  if (launched) return;
  launched = true;
  clearTimeout(fallback);
  if (launch !== "armed") return;
  launch = "running";
  // Held until the cascade's last beat has landed: 14 beats of 20ms, then a
  // page-length settle. Clearing it earlier would cut rows off mid-rise.
  launchTimer = setTimeout(() => {
    launch = "idle";
  }, 1000);
}

export function dispose() {
  clearTimeout(launchTimer);
  clearTimeout(fallback);
  launch = "idle";
}
