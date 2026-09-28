import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import MemoryPage from "../components/sections/MemoryPage.svelte";
import SitesPage from "../components/sections/SitesPage.svelte";
import SkillsPage from "../components/sections/SkillsPage.svelte";
import WorkContextStage from "./WorkContextStage.svelte";

vi.mock("$domain/tabs", () => ({
  tabs: { profile: () => ({ id: "00000000000000000000000001", kind: "regular" }) },
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  const { memories, sites, skills, skillText } = await import("./work-context-fixtures");
  return mockBindings({
    faviconProbe: async () => true,
    workSites: vi.fn(async (profile: string) => ({ version: 1, profile, sites, error: null })),
    workMemories: vi.fn(async (profile: string) => ({
      version: 1,
      profile,
      memories,
      refused: null,
      error: null,
    })),
    workSkills: vi.fn(async (profile: string) => ({
      version: 1,
      profile,
      skills,
      text: null,
      fault: null,
      error: null,
    })),
    workSkillText: vi.fn(async (profile: string, name: string) => ({
      version: 1,
      profile,
      skills,
      text: skillText(name),
      fault: null,
      error: null,
    })),
  });
});

const shots = "../../../../../target/work-settings";
const settle = (ms = 500) => new Promise((done) => setTimeout(done, ms));
async function shoot(name: string) {
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await settle();
    await page.screenshot({ path: `${shots}/${name}-${theme}.png` });
  }
  document.documentElement.dataset.theme = "dark";
}

test("Settings → Sites", async () => {
  await page.viewport(1000, 900);
  const screen = await render(WorkContextStage, {
    title: "Sites",
    description: "Where the agent works in your own signed-in session.",
    page: SitesPage,
  });
  await shoot("sites");
  await screen.unmount();
});

test("Settings → Memory, at rest and editing a fact", async () => {
  await page.viewport(1000, 1100);
  const screen = await render(WorkContextStage, {
    title: "Memory",
    description: "What the agent remembers about you, and where it learned it.",
    page: MemoryPage,
  });
  await shoot("memory");
  await screen.getByRole("button", { name: /Prefers aisle seats/u }).click();
  await shoot("memory-edit");
  await screen.unmount();
});

test("Settings → Skills, the list, a built-in and the person's own skill open", async () => {
  await page.viewport(1000, 1500);
  const screen = await render(WorkContextStage, {
    title: "Skills",
    description: "What the agent knows how to do, built in and your own.",
    page: SkillsPage,
  });
  await shoot("skills");
  await screen.getByRole("button", { name: /Trip planning/u }).click();
  await settle(300);
  await shoot("skills-builtin");
  await screen.getByRole("button", { name: /Weekly review/u }).click();
  await settle(600);
  await shoot("skills-edit");
  await screen.unmount();
});
