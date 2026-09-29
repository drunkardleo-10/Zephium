import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkModelsV1 } from "$shared/ipc/bindings";
import ModelPicker from "../components/bar/ModelPicker.svelte";
import { PROFILE, models } from "$shared/testing/work-models";

const native = vi.hoisted(() => ({ read: vi.fn(), choose: vi.fn(), more: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    workModels: native.read,
    workChooseModel: native.choose,
    workMoreModels: native.more,
  });
});

const view = (lead: string, chosen: string | null = null): WorkModelsV1 =>
  models({ keys: { anthropic: "valid", open_ai: "set", open_router: "valid" }, lead, chosen });

test("the trigger names the running model, and the menu changes it", async () => {
  await page.viewport(1200, 800);
  native.read.mockResolvedValue(view("anthropic/claude-opus-5-5"));
  native.choose.mockResolvedValue(view("openai/gpt-6-sol", "openai/gpt-6-sol"));
  const onsettings = vi.fn();
  const screen = await render(ModelPicker, { profile: PROFILE, onsettings });

  const trigger = screen.getByRole("button", { name: "Model: Claude Opus 5.5" });
  await expect.element(trigger).toHaveTextContent("Opus 5.5");
  await trigger.click();

  await expect.element(page.getByRole("region", { name: "Anthropic" })).toBeVisible();
  await expect
    .element(page.getByRole("button", { name: "Claude Opus 5.5", exact: true }))
    .toHaveAttribute("aria-current", "true");
  // Google and DeepSeek have no key: they read as needing one.
  await expect.element(page.getByRole("button", { name: /Google.*Needs a key/u })).toBeVisible();

  await page.getByRole("button", { name: "GPT-6 Sol", exact: true }).click();
  expect(native.choose).toHaveBeenCalledWith(PROFILE, "lead", "openai/gpt-6-sol");
  await expect.element(screen.getByRole("button", { name: "Model: GPT-6 Sol" })).toBeVisible();

  await screen.getByRole("button", { name: "Model: GPT-6 Sol" }).click();
  await page.getByRole("button", { name: "Add a key…" }).click();
  expect(onsettings).toHaveBeenCalledTimes(1);
});

test("the menu lists each provider's curated models, the smaller ones too", async () => {
  await page.viewport(1200, 800);
  native.read.mockResolvedValue(view("openai/gpt-6-luna"));
  const screen = await render(ModelPicker, { profile: PROFILE, onsettings: vi.fn() });
  await screen.getByRole("button", { name: "Model: GPT-6 Luna" }).click();
  const openai = page.getByRole("region", { name: "OpenAI" });
  await expect
    .element(openai.getByRole("button", { name: "GPT-6 Luna" }))
    .toHaveAttribute("aria-current", "true");
  await expect.element(openai.getByRole("button", { name: "GPT-6 Astra" })).toBeVisible();
  await expect
    .element(
      page
        .getByRole("region", { name: "Anthropic" })
        .getByRole("button", { name: "Claude Fable 5.1" }),
    )
    .toBeVisible();
  await expect.element(page.getByRole("button", { name: "More models" })).not.toBeInTheDocument();
  expect(native.more).not.toHaveBeenCalled();
});

test("without any key the trigger asks for a model and the menu leads to keys", async () => {
  await page.viewport(1200, 800);
  native.read.mockResolvedValue(models({}));
  const onsettings = vi.fn();
  const screen = await render(ModelPicker, { profile: PROFILE, onsettings });
  await screen.getByRole("button", { name: "Model" }).click();
  await expect.element(page.getByRole("button", { name: /Anthropic.*Needs a key/u })).toBeVisible();
  await page.getByRole("button", { name: /Anthropic.*Needs a key/u }).click();
  expect(onsettings).toHaveBeenCalledTimes(1);
});
