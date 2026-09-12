import { expect, test } from "vitest";
import { artifactRenderable, type ArtifactView } from "../artifact";
test("bounds aggregate comparison text even when every cell fits individually", () => {
  const view: ArtifactView = {
    key: "comparison",
    title: "Options",
    reviewLabel: "Needs review",
    evidence: [],
    content: {
      kind: "comparison",
      criteria: ["Description"],
      alternatives: Array.from({ length: 100 }, (_, i) => ({
        name: String(i),
        values: ["x".repeat(4096)],
      })),
    },
  };
  expect(artifactRenderable(view)).toBe(false);
});
