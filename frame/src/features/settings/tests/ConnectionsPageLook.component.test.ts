import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import type {
  WorkConnectionsResponseV1,
  WorkServerCheckV1,
  WorkServerRowV1,
} from "$shared/ipc/bindings";
import ConnectionsStage from "./ConnectionsStage.svelte";

const PROFILE = "00000000000000000000000001";

const servers: WorkServerRowV1[] = [
  {
    server: {
      id: "linear",
      name: "Linear",
      transport: { kind: "http", url: "https://mcp.linear.app/mcp", auth: "oauth" },
      enabled: true,
    },
    secrets: [],
    signed_in: true,
  },
  {
    server: {
      id: "notion",
      name: "Notion",
      transport: { kind: "http", url: "https://mcp.notion.com/mcp", auth: "oauth" },
      enabled: true,
    },
    secrets: [],
    signed_in: false,
  },
  {
    server: {
      id: "files",
      name: "Project files",
      transport: {
        kind: "stdio",
        command: "npx",
        args: ["-y", "@modelcontextprotocol/server-filesystem", "~/Projects"],
        env: [],
      },
      enabled: true,
    },
    secrets: [],
    signed_in: false,
  },
  {
    server: {
      id: "postgres",
      name: "Analytics DB",
      transport: {
        kind: "stdio",
        command: "uvx",
        args: ["mcp-server-postgres"],
        env: [{ name: "DATABASE_URL", secret: true, value: null }],
      },
      enabled: false,
    },
    secrets: ["env.DATABASE_URL"],
    signed_in: false,
  },
];

const listing: WorkConnectionsResponseV1 = {
  version: 1,
  profile: PROFILE,
  clis: [
    { id: "gh", status: "signed_in", version: "2.97.0", account: "crynta" },
    { id: "git", status: "ready", version: "2.54.0", account: "Ada Lovelace" },
    { id: "codex", status: "signed_out", version: "0.153.4", account: null },
    { id: "claude", status: "missing", version: null, account: null },
  ],
  servers,
  error: null,
};

const check = (id: string, outcome: WorkServerCheckV1["outcome"]): WorkServerCheckV1 => ({
  version: 1,
  profile: PROFILE,
  id,
  outcome,
  server_name: null,
  tools:
    outcome === "ready"
      ? [
          { name: "search_issues", title: "Search issues", asks: false },
          { name: "get_issue", title: "Get issue", asks: false },
          { name: "create_issue", title: "Create issue", asks: true },
          { name: "update_issue", title: "Update issue", asks: true },
          { name: "list_projects", title: "List projects", asks: false },
        ]
      : [],
  error: null,
});

vi.mock("$domain/tabs", () => ({
  tabs: { profile: () => ({ id: "00000000000000000000000001", kind: "regular" }) },
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    workConnections: vi.fn(async () => listing),
    workCheckConnection: vi.fn(async (_profile: string, id: string) =>
      check(id, id === "files" ? "not_found" : "ready"),
    ),
  });
});

const shots = "../../../../../target/work-connections";
const settle = () => new Promise((done) => setTimeout(done, 400));

async function shoot(name: string) {
  for (const theme of ["dark", "light"]) {
    document.documentElement.dataset.theme = theme;
    await settle();
    await page.screenshot({ path: `${shots}/${name}-${theme}.png` });
  }
}

test("Settings → Connections: tools on this Mac and servers, in both themes", async () => {
  await page.viewport(1100, 1180);
  const screen = await render(ConnectionsStage);
  await expect.element(screen.getByText("Signed in as crynta")).toBeVisible();
  await screen.getByRole("button", { name: "Test" }).first().click();
  await screen.getByRole("button", { name: "Test" }).nth(1).click();
  await shoot("settings");

  await screen.getByRole("button", { name: "Add…" }).click();
  await screen.getByRole("button", { name: "Sentry" }).click();
  await shoot("settings-add");

  await screen.getByRole("radio", { name: "On this Mac" }).click();
  await screen.getByRole("button", { name: "Add variable" }).click();
  await screen.getByRole("button", { name: "Save" }).click();
  await shoot("settings-add-command");
  document.documentElement.dataset.theme = "dark";
});
