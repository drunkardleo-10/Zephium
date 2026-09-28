import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkModelProvider } from "$shared/ipc/bindings";
import { PROFILE } from "$shared/testing/work-models";
import ModelPickerStage from "./ModelPickerStage.svelte";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  const { models } = await import("$shared/testing/work-models");
  return mockBindings({
    workModels: vi.fn(async () =>
      models({
        keys: { anthropic: "valid", open_ai: "valid", open_router: "set" },
        lead: "anthropic/claude-opus-5-5",
      }),
    ),
    workMoreModels: vi.fn(async (_profile: string, provider: WorkModelProvider) => ({
      provider,
      entries: [],
      fault: null,
    })),
  });
});

const shots = "../../../../../target/work-models";
const settle = () => new Promise((done) => setTimeout(done, 450));

test("the model picker in the ask bar, closed, open and on More models, in both themes", async () => {
  await page.viewport(1100, 700);
  const screen = await render(ModelPickerStage, { profile: PROFILE });
  const shoot = async (name: string) => {
    for (const theme of ["dark", "light"]) {
      document.documentElement.dataset.theme = theme;
      await settle();
      await page.screenshot({ path: `${shots}/picker-${name}-${theme}.png` });
    }
    document.documentElement.dataset.theme = "dark";
  };
  await shoot("rest");
  await screen.getByRole("button", { name: "Model: Claude Opus 5.5" }).click();
  await shoot("open");
  await page.getByRole("button", { name: "More models" }).click();
  await shoot("more");
});
