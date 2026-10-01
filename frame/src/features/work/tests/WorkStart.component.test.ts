import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import StartStage from "./StartStage.svelte";

const { pickFolder } = vi.hoisted(() => ({
  pickFolder: vi.fn(async () => ({
    status: "ok" as const,
    data: { kind: "admitted" as const, path: "/Users/me/Dev/tidepool", name: "tidepool" },
  })),
}));

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workPickFolder: pickFolder });
});

const field = () => page.getByRole("textbox").element() as HTMLTextAreaElement;

test("a tap writes the workflow's request into the ask bar and asks for its one input", async () => {
  await page.viewport(1280, 820);
  render(StartStage, { works: [] });
  await expect
    .element(page.getByRole("heading", { name: "What should we get done?" }))
    .toBeVisible();
  await expect.element(page.getByText("For you")).not.toBeInTheDocument();

  const skill = () => document.querySelector<HTMLElement>("[data-skill]")?.dataset.skill;
  await page.getByRole("button", { name: /Map a project/u }).click();
  expect(field().value).toBe("Map the architecture of ");
  await expect.poll(skill).toBe("map-a-project");
  expect(document.activeElement).toBe(field());
  await expect
    .element(page.getByText("Type the project's path, or choose its folder."))
    .toBeVisible();

  await page.getByRole("button", { name: "Choose a folder" }).click();
  await expect.poll(() => field().value).toBe("Map the architecture of /Users/me/Dev/tidepool");
  expect(pickFolder).toHaveBeenCalledWith("p");

  await page.getByRole("radio", { name: "Manager" }).click();
  await page.getByRole("button", { name: /Plan my day/u }).click();
  expect(field().value).toBe("Plan my day");
  await expect
    .element(page.getByText("Press Return to start, or add what matters first."))
    .toBeVisible();

  await userEvent.fill(page.getByRole("textbox"), "something else");
  await expect
    .element(page.getByText("Press Return to start, or add what matters first."))
    .not.toBeInTheDocument();
  await expect.poll(skill).toBe("");
});

test("words already in the bar become the workflow's input", async () => {
  await page.viewport(1280, 820);
  render(StartStage, { works: [], value: "the SaaS billing market" });
  await page.getByRole("radio", { name: "Founder" }).click();
  await page.getByRole("button", { name: /Market map/u }).click();
  expect(field().value).toBe("Map the market for the SaaS billing market");
  await page.getByRole("button", { name: /Pricing research/u }).click();
  expect(field().value).toBe("Research how competitors price the SaaS billing market");
});

test("for you: the workflows of the latest works, and their role opens first", async () => {
  await page.viewport(1280, 820);
  render(StartStage, {
    works: [
      { requests: ["Make a moodboard for calm fintech"], touched_ms: "300" },
      { requests: ["Plan my day"], touched_ms: "200" },
    ],
  });
  await expect.element(page.getByText("For you")).toBeVisible();
  await expect.element(page.getByRole("radio", { name: "Designer" })).toBeChecked();
  const recent = page.getByRole("button", { name: "Plan my day", exact: true });
  await recent.click();
  expect(field().value).toBe("Plan my day");
  await expect.element(page.getByRole("radio", { name: "Manager" })).toBeChecked();
});
