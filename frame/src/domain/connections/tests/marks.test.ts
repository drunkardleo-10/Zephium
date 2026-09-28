import { describe, expect, test } from "vitest";
import type { WorkServerV1 } from "$shared/ipc/bindings";
import { serverKey, serviceKey, serviceMark, serviceName } from "../marks";

const stdio = (id: string, name: string, args: string[]): WorkServerV1 => ({
  id,
  name,
  transport: { kind: "stdio", command: "npx", args, env: [] },
  enabled: true,
});

describe("service marks", () => {
  test("tools and hosts name their service", () => {
    expect(serviceKey("gh")).toBe("github");
    expect(serviceKey("git")).toBe("git");
    expect(serviceKey("codex")).toBe("codex");
    expect(serviceKey("claude")).toBe("claude");
    expect(serviceKey("github.com")).toBe("github");
    expect(serviceKey("mail.google.com", "Gmail")).toBe("gmail");
    expect(serviceKey("something else")).toBe("other");
  });

  test("a server is known by its package or address, whatever its id", () => {
    expect(serverKey(stdio("work-notes", "Team wiki", ["-y", "@notionhq/notion-mcp-server"]))).toBe(
      "notion",
    );
    expect(
      serverKey({
        id: "tracker",
        name: "Tracker",
        transport: { kind: "http", url: "https://mcp.linear.app/mcp", auth: "oauth" },
        enabled: true,
      }),
    ).toBe("linear");
    expect(serverKey(stdio("fs", "Files", ["@modelcontextprotocol/server-filesystem"]))).toBe(
      "files",
    );
  });

  test("every key has a mark, and known services a name", () => {
    expect(serviceMark("linear").length).toBeGreaterThan(0);
    expect(serviceMark("other").length).toBeGreaterThan(0);
    expect(serviceName("github")).toBe("GitHub");
    expect(serviceName("other")).toBeUndefined();
  });
});
