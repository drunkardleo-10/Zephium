import { expect, test } from "vitest";
import type { WorkArtifactDataV1 } from "$shared/ipc/bindings";
import { artifactRenderable } from "$shared/ui/data/Artifact/artifact";
import { resultSize } from "../lib/card-size";
import { artifactView } from "../lib/project-work";
import { projection } from "./environment-fixtures";

function view(data: WorkArtifactDataV1) {
  const execution = structuredClone(projection).executions[0]!;
  execution.artifacts[0]!.data = data;
  execution.user_artifacts = [];
  return artifactView(execution.artifacts[0]!, execution);
}
const lines = (count: number) =>
  Array.from({ length: count }, (_, index) => `let x${index} = ${index};\n`).join("");

test("code projects with its notes, and a note past the text is unavailable", () => {
  const code = view({ kind: "code", language: "sql", text: "select 1;\n" });
  expect(code.content).toEqual({ kind: "code", language: "sql", text: "select 1;\n", notes: [] });
  expect(artifactRenderable(code)).toBe(true);
  const noted = view({
    kind: "code",
    language: "rust",
    text: lines(3),
    notes: [{ from: 2, to: 3, text: "x" }],
  });
  expect(artifactRenderable(noted)).toBe(true);
  const past = view({
    kind: "code",
    language: "rust",
    text: lines(3),
    notes: [{ from: 3, to: 4, text: "x" }],
  });
  expect(artifactRenderable(past)).toBe(false);
});

test("a code card is 420 wide and as tall as its first fourteen lines", () => {
  const short = resultSize("Main", view({ kind: "code", language: "rust", text: lines(4) }));
  const full = resultSize("Main", view({ kind: "code", language: "rust", text: lines(14) }));
  const long = resultSize("Main", view({ kind: "code", language: "rust", text: lines(300) }));
  expect(short.width).toBe(420);
  expect(full.height - short.height).toBeCloseTo(10 * 19.2, 0);
  // Past fourteen lines only the footer grows the card: "+n lines".
  expect(long.height - full.height).toBe(29 - 16);
});
