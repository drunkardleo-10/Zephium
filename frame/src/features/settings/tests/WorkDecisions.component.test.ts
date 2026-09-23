import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkDecisionPreferenceV1 } from "$shared/ipc/bindings";
import WorkDecisions from "../components/WorkDecisions.svelte";

const native = vi.hoisted(() => ({ read: vi.fn(), set: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    workDecisionPreference: native.read,
    workSetDecisionPreference: native.set,
  });
});

const PROFILE = "00000000000000000000000001";
const preference = (
  choice: WorkDecisionPreferenceV1["choice"],
  key: boolean,
): WorkDecisionPreferenceV1 => ({
  version: 1,
  profile: PROFILE,
  choice,
  effective: choice === "recommended" && !key ? "standard" : choice,
  typesafe_key_present: key,
  error: null,
});

test("choosing Standard sets the preference and the disclosure follows what runs", async () => {
  await page.viewport(1200, 800);
  native.read.mockResolvedValue(preference("recommended", true));
  native.set.mockResolvedValue(preference("standard", true));
  const screen = await render(WorkDecisions, { profile: PROFILE });

  await expect
    .element(screen.getByText("Page text is sent to TypeSafe and to OpenAI."))
    .toBeVisible();
  await expect.element(screen.getByRole("radio", { name: "Recommended" })).toBeChecked();

  // The segmented control hides its radios; the person clicks the label.
  await screen.getByText("Standard", { exact: true }).click();

  expect(native.set).toHaveBeenCalledWith(PROFILE, "standard");
  await expect.element(screen.getByText("Page text is sent to OpenAI.")).toBeVisible();
  await expect.element(screen.getByRole("radio", { name: "Standard" })).toBeChecked();
});

test("Recommended without the Keychain item reports the mode that actually runs", async () => {
  await page.viewport(1200, 800);
  native.read.mockResolvedValue(preference("recommended", false));
  const screen = await render(WorkDecisions, { profile: PROFILE });

  await expect.element(screen.getByRole("radio", { name: "Recommended" })).toBeChecked();
  await expect.element(screen.getByText("Active: Standard")).toBeVisible();
  await expect.element(screen.getByText("Page text is sent to OpenAI.")).toBeVisible();
});
