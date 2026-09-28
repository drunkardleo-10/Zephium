import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import SitesPage from "../components/sections/SitesPage.svelte";
import WorkContextStage from "./WorkContextStage.svelte";

const PROFILE = "00000000000000000000000001";
vi.mock("$domain/tabs", () => ({
  tabs: { profile: () => ({ id: "00000000000000000000000001", kind: "regular" }) },
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  const sites = [
    { site: "slack.com", name: "Slack", access: "always", sensitive: false },
    { site: "notion.so", name: "Notion", access: "always", sensitive: false },
    { site: "linkedin.com", name: "LinkedIn", access: "ask", sensitive: false },
    { site: "reddit.com", name: "Reddit", access: "never", sensitive: false },
    { site: "chase.com", name: "chase.com", access: "ask", sensitive: true },
  ];
  return mockBindings({
    faviconProbe: async () => true,
    workSites: vi.fn(async (profile: string) => ({ version: 1, profile, sites, error: null })),
  });
});

const shots = "../../../../../target/work-settings";
const settle = () => new Promise((done) => setTimeout(done, 500));

test.each([["sites", "Sites", "Where the agent works in your own session.", SitesPage]] as const)(
  "Settings → %s in both themes",
  async (name, title, description, section) => {
    void PROFILE;
    await page.viewport(1000, 900);
    const screen = await render(WorkContextStage, { title, description, page: section });
    for (const theme of ["dark", "light"]) {
      document.documentElement.dataset.theme = theme;
      await settle();
      await page.screenshot({ path: `${shots}/${name}-${theme}.png` });
    }
    document.documentElement.dataset.theme = "dark";
    await screen.unmount();
  },
);
