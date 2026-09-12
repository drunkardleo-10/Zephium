import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { resourceTestServer } from "$shared/testing/resources/server";
import { loadTasks } from "../index";
const host = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ resourceCall: host.call });
});
test("captures a task, saves its due date, completes it and restores it from trash", async () => {
  const profile = "00000000000000000000000002";
  const server = resourceTestServer(profile);
  host.call.mockImplementation(server.call);
  const { default: Tasks } = await loadTasks();
  const screen = await render(Tasks, { profile, host: "task-integration" });
  screen.container.style.height = "700px";
  screen.container.style.width = "320px";
  screen.container.style.display = "flex";
  await expect
    .poll(() => {
      const panel = screen.container.querySelector<HTMLElement>(".resource-panel")!;
      return Math.round(panel.getBoundingClientRect().width);
    })
    .toBe(320);
  await screen.getByRole("button", { name: "New", exact: true }).click();
  await screen.getByRole("textbox", { name: "Title", exact: true }).fill("Review the evidence");
  await screen
    .getByRole("textbox", { name: "Description", exact: true })
    .fill("Compare both sources before deciding.");
  await screen.getByLabelText("Due date", { exact: true }).fill("2028-02-29");
  await screen.getByRole("checkbox", { name: "Completed", exact: true }).click();
  await expect
    .poll(() => [...server.records.values()][0]?.draft.content)
    .toEqual({
      kind: "task",
      description: "Compare both sources before deciding.",
      completed: true,
      due_date: "2028-02-29",
    });
  await screen.getByRole("button", { name: "Move to trash", exact: true }).click();
  await expect.poll(() => [...server.records.values()][0]?.trashed).toBe(true);
  await screen.getByRole("button", { name: "Restore", exact: true }).click();
  await expect.poll(() => [...server.records.values()][0]?.trashed).toBe(false);
});
