import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { preferences } from "$domain/preferences";
import PerformancePage from "../components/sections/PerformancePage.svelte";

vi.mock("$domain/preferences", () => ({
  preferences: {
    value: (key: string) =>
      ({
        "performance.sleep": "true",
        "performance.after": "15",
        "performance.memory": "balanced",
        "performance.exceptions": "kept.example",
      })[key],
    saving: () => false,
    saveFailed: () => false,
    set: vi.fn().mockResolvedValue(undefined),
  },
}));

test("performance controls dispatch durable settings and normalize awake sites", async () => {
  const screen = await render(PerformancePage);
  await screen.getByRole("switch", { name: "Put inactive tabs to sleep" }).click();
  expect(preferences.set).toHaveBeenCalledWith("performance.sleep", "false");
  await screen.getByLabelText("URL").fill("https://www.Example.com/a");
  await screen.getByRole("button", { name: "Add", exact: true }).click();
  expect(preferences.set).toHaveBeenCalledWith(
    "performance.exceptions",
    "kept.example\nexample.com",
  );
  await screen.getByRole("button", { name: "Remove kept.example" }).click();
  expect(preferences.set).toHaveBeenCalledWith("performance.exceptions", "");
  expect(screen.container.textContent).not.toContain("Preview");
});
