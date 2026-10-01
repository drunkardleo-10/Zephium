import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import { WorkSession } from "$domain/work";
import PresenceStage from "./PresenceStage.svelte";
import { fourHelpersScene } from "./presence-look";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ faviconProbe: async () => true });
});

// Holds a run with four helpers live, then the same run settled, long enough for the
// WebContent process's CPU to be sampled from outside (`VITE_CPU=1`, see the report).
const hold = Number(import.meta.env.VITE_CPU ?? 0) * 1000;
const running = () => document.getAnimations().filter((each) => each.playState === "running");

test.skipIf(!hold)(
  "four live helpers run on the compositor, and at rest nothing runs",
  async () => {
    await page.viewport(1440, 820);
    const scene = fourHelpersScene();
    const session = new WorkSession("profile");
    session.selected = "objective";
    session.projection = structuredClone([...scene.objectives.values()][0]!);
    const working = [
      { id: "github", title: "GitHub", now: "Reading pull request", helper: "connection" as const },
      { id: "code", title: "Code", now: "Running cargo test", helper: "computer" as const },
      {
        id: "docs",
        title: "Manual",
        now: "Reading",
        host: "cli.github.com",
        helper: "browser" as const,
      },
      { id: "web", title: "Reports", now: "Searching", helper: "research" as const },
    ];
    const screen = await render(PresenceStage, {
      scene,
      session,
      working,
      viewport: { x: 40, y: 90, zoom: 1 },
    });
    screen.container.style.width = "1440px";
    screen.container.style.height = "820px";
    await new Promise((done) => setTimeout(done, 1500));
    const live = running();
    const properties = new Set(
      live.flatMap((animation) =>
        ((animation.effect as KeyframeEffect | null)?.getKeyframes() ?? []).flatMap((frame) =>
          Object.keys(frame).filter(
            (key) => !["offset", "computedOffset", "easing", "composite"].includes(key),
          ),
        ),
      ),
    );
    console.warn(
      `LIVE ${Date.now()} animations=${live.length} properties=${[...properties].join(",")}`,
    );
    for (const property of properties) expect(["transform", "opacity"]).toContain(property);
    await new Promise((done) => setTimeout(done, hold));
    console.warn(`LIVE-END ${Date.now()}`);
    await screen.unmount();
    session.dispose();
    const settled = fourHelpersScene();
    for (const projection of settled.objectives.values())
      for (const execution of projection.executions) {
        execution.status = "completed";
        execution.steps = (execution.steps ?? []).map((step) => ({ ...step, status: "succeeded" }));
        execution.parts = (execution.parts ?? []).map((part) => ({ ...part, state: "done" }));
      }
    const quiet = new WorkSession("profile");
    quiet.selected = "objective";
    quiet.projection = structuredClone([...settled.objectives.values()][0]!);
    const rest = await render(PresenceStage, {
      scene: settled,
      session: quiet,
      working: [],
      viewport: { x: 40, y: 90, zoom: 1 },
    });
    rest.container.style.width = "1440px";
    rest.container.style.height = "820px";
    await new Promise((done) => setTimeout(done, 2500));
    console.warn(`REST ${Date.now()} animations=${running().length}`);
    expect(running()).toHaveLength(0);
    await new Promise((done) => setTimeout(done, hold));
    console.warn(`REST-END ${Date.now()}`);
    await rest.unmount();
    quiet.dispose();
  },
  2 * hold + 30_000,
);
