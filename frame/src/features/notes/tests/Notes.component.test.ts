import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { userEvent } from "vitest/browser";
import { resourceTestServer } from "$shared/testing/resources/server";
import { loadNotes } from "../index";
import type { ResourceDraft, ResourceRecord } from "$domain/resources";
const host = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ resourceCall: host.call });
});
test("creates a durable-identity note and saves a constrained rich document", async () => {
  const profile = "00000000000000000000000001";
  const server = resourceTestServer(profile);
  host.call.mockImplementation(server.call);
  const { default: Notes } = await loadNotes();
  const screen = await render(Notes, { profile, host: "note-integration" });
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
  await screen.getByRole("textbox", { name: "Title", exact: true }).fill("A useful finding");
  await screen.getByRole("textbox", { name: "Notes", exact: true }).fill("Observed evidence");
  await userEvent.keyboard(
    navigator.platform.includes("Mac") ? "{Meta>}a{/Meta}" : "{Control>}a{/Control}",
  );
  await screen.getByRole("button", { name: "Bold", exact: true }).click();
  await expect.poll(() => [...server.records.values()][0]?.draft.title).toBe("A useful finding");
  await expect
    .poll(() => JSON.stringify([...server.records.values()][0]?.draft.content))
    .toContain('"type":"bold"');
  const record: ResourceRecord = [...server.records.values()][0]!;
  const draft: ResourceDraft = record.draft;
  expect(draft.content.kind).toBe("note");
  expect(record.id).toHaveLength(26);
  await screen.getByRole("button", { name: "Move to trash", exact: true }).click();
  await expect.poll(() => server.records.get(record.id)?.trashed).toBe(true);
  await screen.getByRole("button", { name: "Restore", exact: true }).click();
  await expect.poll(() => server.records.get(record.id)?.trashed).toBe(false);
});
