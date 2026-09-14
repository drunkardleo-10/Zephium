import { expect, test, vi } from "vitest";
import { userEvent } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import { tabFixture } from "$shared/testing/fixtures";
import WorkTabPicker from "../components/WorkTabPicker.svelte";
import WorkEnvironmentHarness from "./WorkEnvironmentHarness.svelte";

test("tab selection only requests exact retained identities, with projection-owned settlement", async () => {
  const onattach = vi.fn();
  const onopen = vi.fn();
  const onnewtab = vi.fn();
  const tabs = [
    tabFixture({ id: "retained-one", title: "First page" }),
    tabFixture({ id: "retained-two", title: "Other page", url: null }),
  ];
  const screen = await render(WorkTabPicker, {
    tabs,
    spaceName: "Personal",
    onattach,
    onopen,
    onnewtab,
  });
  expect(onattach).not.toHaveBeenCalled();
  expect(onopen).not.toHaveBeenCalled();
  await screen.getByRole("checkbox", { name: /First page/ }).click();
  await screen.getByRole("button", { name: "Add to Work (1)" }).click();
  expect(onattach).toHaveBeenCalledExactlyOnceWith(["retained-one"]);
  await expect.element(screen.getByRole("checkbox", { name: /First page/ })).toBeEnabled();
  await screen.rerender({ pending: true, status: "Waiting for native confirmation" });
  await expect
    .element(screen.getByRole("button", { name: "Waiting for confirmation…" }))
    .toBeDisabled();
  await screen.rerender({ pending: false, attachedTabIds: ["retained-one"], status: "" });
  await expect.element(screen.getByRole("checkbox", { name: /First page/ })).toBeDisabled();
  await screen.getByRole("button", { name: "Open Other page here" }).click();
  expect(onopen).toHaveBeenCalledExactlyOnceWith("retained-two");
  await screen.getByRole("button", { name: "New tab" }).click();
  expect(onnewtab).toHaveBeenCalledOnce();
});

test("search preserves selection, while a closed tab cannot be attached", async () => {
  const onattach = vi.fn();
  const tabs = [
    tabFixture({ id: "one", title: "First page" }),
    tabFixture({ id: "two", title: "Other page" }),
  ];
  const screen = await render(WorkTabPicker, {
    tabs,
    spaceName: "Personal",
    onattach,
    onopen: vi.fn(),
    onnewtab: vi.fn(),
  });
  await screen.getByRole("checkbox", { name: /First page/ }).click();
  await screen.getByRole("searchbox").fill("Other");
  await expect.element(screen.getByRole("button", { name: "Add to Work (1)" })).toBeEnabled();
  await screen.rerender({ tabs: [tabs[1]!] });
  await expect.element(screen.getByRole("button", { name: "Add to Work (0)" })).toBeDisabled();
  expect(onattach).not.toHaveBeenCalled();
});

test("manual Work starts with a dismissible tab picker and accessible return control", async () => {
  const onreturn = vi.fn();
  const onpanelchange = vi.fn();
  const screen = await render(WorkEnvironmentHarness, { onreturn, onpanelchange });
  await expect.element(screen.getByRole("textbox", { name: "Find a tab" })).toBeVisible();
  await userEvent.keyboard("{Escape}");
  await expect.element(screen.getByRole("button", { name: "Tabs", exact: true })).toHaveFocus();
  await expect.element(screen.getByRole("button", { name: "Notes", exact: true })).toBeDisabled();
  await screen.getByRole("button", { name: "Canvas object" }).click();
  await screen.getByRole("button", { name: "Switch Space or Work" }).click();
  await screen.getByRole("button", { name: "Return to Browse" }).click();
  expect(onreturn).toHaveBeenCalledOnce();
  expect(onpanelchange).toHaveBeenCalledWith(null);
});
