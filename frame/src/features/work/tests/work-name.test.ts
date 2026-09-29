import { describe, expect, test } from "vitest";
import type { WorkEnvironmentSummary, WorkRuntimeProjection } from "$shared/ipc/bindings";
import { defaultTitle, firstRunName, listedName, requestName } from "../lib/work-name";

const work = (over: Partial<WorkEnvironmentSummary>): WorkEnvironmentSummary => ({
  id: "01M3PGBTDZVSAZZ8MBTKGXXDR9",
  space: "01M3CTWDG20GFZPACD7905RSRJ",
  title: "New work",
  lifecycle: "active",
  revision: "1",
  name: null,
  requests: [],
  touched_ms: "0",
  empty: false,
  ...over,
});

describe("a work's name", () => {
  test("comes from the first words, whole, never ending on a joining word", () => {
    expect(requestName("Plan my whole trip for YC batch from Poland to SF as a solo founder")).toBe(
      "Plan my whole trip for YC batch",
    );
    expect(requestName("compare SQLite, DuckDB and RocksDB for an embedded cache")).toBe(
      "Compare SQLite, DuckDB and RocksDB",
    );
    expect(requestName("What's in ~/Dev/Lunios")).toBe("What's in ~/Dev/Lunios");
    expect(requestName("Plan my whole day. Use Slack and Gmail.")).toBe("Plan my whole day");
  });

  test("the list prefers the person's name, then the first run's, then the first words", () => {
    expect(listedName(work({ title: "Taxes" }), "New work")).toBe("Taxes");
    expect(listedName(work({ name: "Compiler learning plan" }), "New work")).toBe(
      "Compiler learning plan",
    );
    expect(
      listedName(work({ title: "Untitled project", requests: ["Plan my day"] }), "New work"),
    ).toBe("Plan my day");
    expect(defaultTitle("  ", "New work")).toBe(true);
  });
});

describe("a work named by its first request", () => {
  const run = (title: string | null, reply?: string) =>
    ({
      ...(title ? { title } : {}),
      artifacts: reply ? [{ title: reply, data: { kind: "reply" } }] : [],
    }) as unknown as WorkRuntimeProjection["executions"][number];
  const work = (...executions: WorkRuntimeProjection["executions"]) =>
    ({ executions }) as unknown as WorkRuntimeProjection;

  test("takes the first title a run gave, else the first reply's, else the first words", () => {
    expect(
      firstRunName(work(run(null, "Context so far"), run("YC trip from Warsaw")), "Plan it", false),
    ).toBe("YC trip from Warsaw");
    expect(firstRunName(work(run(null, "Your compiler learning path")), "Find", false)).toBe(
      "Your compiler learning path",
    );
    expect(firstRunName(work(run(null)), "plan my whole day", false)).toBe("Plan my whole day");
  });

  test("waits while the first run is still going", () => {
    expect(firstRunName(work(run("Trip")), "Plan", true)).toBe("");
  });
});
