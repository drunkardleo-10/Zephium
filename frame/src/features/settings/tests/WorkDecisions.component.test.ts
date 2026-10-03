import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkDecisionPreferenceV1 } from "$shared/ipc/bindings";
import WorkDecisions from "../components/WorkDecisions.svelte";

const native = vi.hoisted(() => ({ read: vi.fn(), set: vi.fn(), key: vi.fn(), clear: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    workDecisionPreference: native.read,
    workSetDecisionPreference: native.set,
    workSetDecisionKey: native.key,
    workClearDecisionKey: native.clear,
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

test("Recommended without the OS credential item reports the mode that actually runs", async () => {
  await page.viewport(1200, 800);
  native.read.mockResolvedValue(preference("recommended", false));
  const screen = await render(WorkDecisions, { profile: PROFILE });

  await expect.element(screen.getByRole("radio", { name: "Recommended" })).toBeChecked();
  await expect.element(screen.getByText("Active: Standard")).toBeVisible();
  await expect.element(screen.getByText("Page text is sent to OpenAI.")).toBeVisible();
});

test("an unconfirmed or wrong-profile save never claims a saved Jev credential", async () => {
  native.read.mockResolvedValue(preference("recommended", false));
  native.key.mockResolvedValueOnce({ ...preference("recommended", true), profile: "other" });
  native.key.mockRejectedValueOnce(new Error("native transport refused"));
  const screen = await render(WorkDecisions, { profile: PROFILE });
  await screen.getByRole("button", { name: "Add Jev key" }).click();
  await screen.getByLabelText("Jev (TypeSafe) API key").fill("fixed-nonsecret-fixture");
  await screen.getByRole("button", { name: "Save", exact: true }).click();
  await expect
    .element(screen.getByRole("alert"))
    .toHaveTextContent("couldn’t complete this change");
  await expect.element(screen.getByText("Active: Standard")).toBeVisible();
  await screen.getByLabelText("Jev (TypeSafe) API key").fill("fixed-nonsecret-fixture");
  await screen.getByRole("button", { name: "Save", exact: true }).click();
  await expect
    .element(screen.getByRole("alert"))
    .toHaveTextContent("Couldn’t confirm this key change");
  await expect.element(screen.getByText("Key saved", { exact: true })).not.toBeInTheDocument();
});

test("saving and removing Jev updates Recommended without returning a saved secret", async () => {
  native.read.mockResolvedValue(preference("recommended", false));
  native.key.mockResolvedValue(preference("recommended", true));
  native.clear.mockResolvedValue(preference("recommended", false));
  const screen = await render(WorkDecisions, { profile: PROFILE });
  await screen.getByRole("button", { name: "Add Jev key" }).click();
  const input = screen.getByLabelText("Jev (TypeSafe) API key");
  await expect.element(input).toHaveAttribute("type", "password");
  await input.fill("fixed-nonsecret-jev-fixture");
  await screen.getByRole("button", { name: "Save", exact: true }).click();
  expect(native.key).toHaveBeenCalledWith(PROFILE, "fixed-nonsecret-jev-fixture");
  await expect.element(screen.getByText("Key saved", { exact: true })).toBeVisible();
  await expect.element(screen.getByText("Active: Standard")).not.toBeInTheDocument();
  await screen.getByRole("button", { name: "Replace key…" }).click();
  await expect.element(screen.getByLabelText("Jev (TypeSafe) API key")).toHaveValue("");
  await screen.getByRole("button", { name: "Cancel" }).click();
  await screen.getByRole("button", { name: "Remove key" }).click();
  expect(native.clear).toHaveBeenCalledWith(PROFILE);
  await expect.element(screen.getByText("Active: Standard")).toBeVisible();
});

test("a refused save clears the draft and preserves the last confirmed mode", async () => {
  native.read.mockResolvedValue(preference("recommended", false));
  native.key.mockResolvedValue({ ...preference("recommended", false), error: "invalid" });
  const screen = await render(WorkDecisions, { profile: PROFILE });
  await screen.getByRole("button", { name: "Add Jev key" }).click();
  await screen.getByLabelText("Jev (TypeSafe) API key").fill("invalid fixture");
  await screen.getByRole("button", { name: "Save", exact: true }).click();
  await expect
    .element(screen.getByRole("alert"))
    .toHaveTextContent("Enter a valid TypeSafe API key without spaces.");
  await expect.element(screen.getByLabelText("Jev (TypeSafe) API key")).toHaveValue("");
  await expect.element(screen.getByText("Active: Standard")).toBeVisible();
  await expect.element(screen.getByText("Key saved", { exact: true })).not.toBeInTheDocument();
});
