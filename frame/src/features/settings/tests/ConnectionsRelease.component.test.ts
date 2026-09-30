import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type { WorkServerCheckV1, WorkServerRowV1 } from "$shared/ipc/bindings";
import { ConnectionsSession } from "$domain/connections";
import ServerEditor from "../components/connections/ServerEditor.svelte";
import ServerRow from "../components/connections/ServerRow.svelte";
import CliRow from "../components/connections/CliRow.svelte";

const profile = "00000000000000000000000001";
const row: WorkServerRowV1 = {
  server: {
    id: "notes",
    name: "Notes",
    enabled: true,
    transport: { kind: "http", url: "https://example.com/mcp", auth: "oauth" },
  },
  secrets: [],
  signed_in: false,
};
const check: WorkServerCheckV1 = {
  version: 1,
  profile,
  id: "notes",
  outcome: "ready",
  server_name: "Notes",
  tools: [{ name: "read_notes", title: "Read notes", asks: false }],
  error: null,
};

test("a draft previews tools before Save and edits require another check", async () => {
  await page.viewport(1100, 1100);
  const save = vi.fn();
  const preview = vi.fn(async () => check);
  const screen = await render(ServerEditor, {
    editing: row,
    taken: [],
    saving: false,
    onpreview: preview,
    onsave: save,
    oncancel: vi.fn(),
  });
  await screen.getByRole("button", { name: "Check connection" }).click();
  await expect.element(screen.getByText("Read notes")).toBeVisible();
  expect(save).not.toHaveBeenCalled();
  await screen.getByRole("button", { name: "Save" }).click();
  expect(save).toHaveBeenCalledTimes(1);
  await screen.getByLabelText("Name", { exact: true }).fill("Changed notes");
  await expect.element(screen.getByRole("button", { name: "Check connection" })).toBeVisible();
});

test("cancelled OAuth is recoverable and failure is never shown connected", async () => {
  const session = new ConnectionsSession(profile);
  session.checks = { notes: { ...check, outcome: "cancelled", tools: [] } };
  const screen = await render(ServerRow, { session, row, taken: [] });
  await expect
    .element(screen.getByText("Sign-in cancelled. Try again when you’re ready."))
    .toBeVisible();
  await expect.element(screen.getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
  await screen.unmount();
  session.dispose();
});

test("a detected CLI shows its version and exact path; missing tools offer installation", async () => {
  const found = await render(CliRow, {
    id: "git",
    cli: {
      id: "git",
      status: "ready",
      version: "2.54.0",
      path: "/opt/homebrew/bin/git",
      account: null,
    },
  });
  await expect.element(found.getByText("/opt/homebrew/bin/git")).toBeVisible();
  await expect.element(found.getByText("2.54.0")).toBeVisible();
  await found.unmount();
  const missing = await render(CliRow, {
    id: "claude",
    cli: { id: "claude", status: "missing", version: null, account: null },
  });
  await expect.element(missing.getByRole("button", { name: "Install" })).toBeVisible();
});
