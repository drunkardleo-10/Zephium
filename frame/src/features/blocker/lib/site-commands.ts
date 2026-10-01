import { blocker, blockerSites, hiding } from "$domain/blocker";
import { commands } from "$shared/ipc/bindings";
import { expandBriefly } from "$session/sidebar-mode.svelte";

// Requests the page already made stay made, so the page reloads with the change.
export async function toggleSiteProtection(): Promise<void> {
  const status = blocker.status();
  if (status.applied_enabled !== true) {
    await blocker.setEnabled(true);
    return;
  }
  const site = status.site;
  if (!site?.ready || site.busy) return;
  const result = await blockerSites.changeSite(site.context, {
    kind: "pause",
    paused: !site.paused,
  });
  if (result.state === "processed") void commands.tabsReload(site.context.tab);
}

// The rail has no room for the hiding bar, so the column opens for the
// session and folds back when it ends.
export async function hideElements(): Promise<void> {
  const context = blocker.status().site?.context;
  if (!context) return;
  const restore = expandBriefly();
  if (!(await hiding.start(context, restore))) restore();
}
