import type { SidebarNodeView, TabView } from "$shared/ipc/bindings";
import { tabFixture } from "$shared/testing/fixtures";
import { emitNativeEvent } from "$shared/testing/native-events";

/**
 * A tabs projection that behaves as native does for onboarding: keeping a
 * catalog site adds an unloaded Essentials tab and delivers the site's seeded
 * raster, closing one removes it.
 */
const URLS: Record<string, string> = {
  slack: "https://app.slack.com/client",
  notion: "https://www.notion.so/",
  github: "https://github.com/",
  figma: "https://www.figma.com/files",
  youtube: "https://www.youtube.com/",
  spotify: "https://open.spotify.com/",
  gmail: "https://mail.google.com/",
  linear: "https://linear.app/",
};
const PROFILE = "00000000000000000000000777";

const open = ["New Tab"].map((title, index) =>
  tabFixture({ id: `t${index}`, title, url: null, icon: null }),
);
let kept = $state<TabView[]>([]);
let name = $state("Personal");
let next = 0;

const node = (id: string, section: "favorites" | "today"): SidebarNodeView => ({
  id,
  parent_id: null,
  section,
  kind: { type: "tab", tab_id: id },
});

async function seed(origin: string, site: string) {
  const bytes = new Uint8Array(
    await (
      await fetch(new URL(`../../../../../assets/kept-sites/${site}.rgba`, import.meta.url))
    ).arrayBuffer(),
  );
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  emitNativeEvent("favicons", {
    surface: "chrome",
    profile_id: PROFILE,
    entries: [{ origin, revision: site, rgba: btoa(binary) }],
  });
}

export const fakeTabs = {
  init: async () => {},
  dispose() {},
  profile: () => ({ id: PROFILE, name, kind: "default" as const }),
  tabs: () => [...kept, ...open],
  sidebarNodes: () => [
    ...kept.map((tab) => node(tab.id, "favorites")),
    ...open.map((tab) => node(tab.id, "today")),
  ],
  activeId: () => "t0",
  activeSpaceId: () => "space",
  splitGroup: () => null,
  dropTab() {},
  async keepSite(site: string) {
    const url = URLS[site];
    if (!url) return { outcome: "rejected" as const, disposition: null };
    const origin = url.split("/").slice(0, 3).join("/");
    await seed(origin, site);
    kept = [
      ...kept,
      tabFixture({ id: `k${next++}`, title: site, url, icon: { origin, revision: site } }),
    ];
    return { outcome: "applied" as const, disposition: null };
  },
  close(id: string) {
    kept = kept.filter((tab) => tab.id !== id);
  },
  async renameProfile(next: string) {
    name = next;
    return { outcome: "applied" as const, disposition: null };
  },
  openTabMenu() {},
  reset() {
    kept = [];
    name = "Personal";
  },
};
