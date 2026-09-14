import { expect, test } from "vitest";
import { projection, snapshot } from "./environment-fixtures";
import { environmentItems } from "../lib/project-environment";
test("joins exact objective and historical artifact identities without replacing originals", () => {
  const items = environmentItems(snapshot, [], [], new Map([["objective", projection]]));
  expect(items[0]?.title).toBe("Investigate dependencies");
  expect(items[1]?.artifact?.content).toEqual({
    kind: "document",
    paragraphs: ["Reviewed findings"],
  });
  expect(projection.executions[0]?.artifacts[0]?.data).toEqual({
    kind: "document",
    paragraphs: ["Original evidence"],
  });
  const wrongExecution = {
    ...snapshot,
    elements: [
      {
        ...snapshot.elements[1]!,
        reference: {
          kind: "artifact" as const,
          objective: "objective",
          execution: "other",
          artifact: "artifact",
        },
      },
    ],
  };
  expect(
    environmentItems(wrongExecution, [], [], new Map([["objective", projection]]))[0]?.artifact,
  ).toBeUndefined();
});

test("objective status follows the latest durable execution and native interruption truth", () => {
  const completed = projection.executions[0]!;
  const failed = { ...completed, id: "latest", status: "failed" as const };
  const items = (state: typeof projection) =>
    environmentItems(snapshot, [], [], new Map([["objective", state]]));
  expect(items({ ...projection, executions: [completed, failed] })[0]?.detail).toBe(
    "Status: Failed",
  );
  expect(
    items({
      ...projection,
      executions: [{ ...failed, status: "running" }],
      interrupted: ["latest"],
    })[0]?.detail,
  ).toBe("Status: Interrupted");
  expect(items({ ...projection, executions: [] })[0]?.detail).toBe("Status: Plan ready");
});
