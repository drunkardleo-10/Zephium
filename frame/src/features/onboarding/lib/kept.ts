import type { SidebarNodeView, TabView } from "$shared/ipc/bindings";

/** Which catalog sites are already in Essentials, and as which tab, read from
 *  the projection alone so the grid can never claim a site native did not
 *  keep. A site counts only at the exact address native keeps it at. */
export function keptSites(
  nodes: readonly SidebarNodeView[],
  tabs: readonly TabView[],
  urls: Readonly<Record<string, string>>,
): Map<string, string> {
  const byUrl = new Map<string, string>();
  for (const [site, url] of Object.entries(urls)) byUrl.set(url, site);
  const urlOf = new Map(tabs.map((tab) => [tab.id, tab.url]));
  const kept = new Map<string, string>();
  for (const node of nodes) {
    if (node.section !== "favorites" || node.kind.type !== "tab") continue;
    const url = urlOf.get(node.kind.tab_id);
    const site = url ? byUrl.get(url) : undefined;
    if (site && !kept.has(site)) kept.set(site, node.kind.tab_id);
  }
  return kept;
}
