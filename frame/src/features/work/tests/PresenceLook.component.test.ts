import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import { WorkSession } from "$domain/work";
import PresenceStage from "./PresenceStage.svelte";
import { fourHelpersScene } from "./presence-look";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ faviconProbe: async () => true });
});

const shots = "../../../../../target/work-presence";
const working = [
  {
    id: "github",
    title: "GitHub",
    now: "Reading pull request #14543",
    helper: "connection" as const,
  },
  {
    id: "code",
    title: "Code",
    now: "Running cargo test -p zephium-core duration",
    helper: "computer" as const,
  },
  {
    id: "docs",
    title: "Manual",
    now: "Reading gh repo fork",
    host: "cli.github.com",
    helper: "browser" as const,
  },
  { id: "web", title: "Reports", now: "Searching for the same flag", helper: "research" as const },
];

test("the island and a live run with four helpers on the canvas, in both themes", async () => {
  await page.viewport(1440, 820);
  const scene = fourHelpersScene();
  const session = new WorkSession("profile");
  session.selected = "objective";
  session.projection = structuredClone([...scene.objectives.values()][0]!);
  const screen = await render(PresenceStage, {
    scene,
    session,
    working,
    viewport: { x: 40, y: 90, zoom: 1 },
  });
  screen.container.style.width = "1440px";
  screen.container.style.height = "820px";
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await new Promise((done) => setTimeout(done, 900));
    await page.screenshot({ path: `${shots}/live-${theme}.png` });
  }
  await userEvent.click(screen.getByRole("button", { name: "Agents in this run" }).last());
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await new Promise((done) => setTimeout(done, 500));
    await page.screenshot({ path: `${shots}/island-helpers-${theme}.png` });
  }
  document.documentElement.dataset.theme = "dark";
  await screen.unmount();
  session.dispose();
});
