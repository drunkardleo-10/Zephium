import { expect, test } from "vitest";
import type { WorkArtifactV1, WorkExecutionFact } from "$domain/work";
import { artifactView } from "../lib/project-work";

const citation = (url: string, title: string) => ({ url, title, start_index: 0, end_index: 1 });

test("a source is named by its title, then its site, then its kind; never by a number", () => {
  const execution = {
    id: "execution",
    provider_evidence: [
      {
        id: "search",
        node: "node",
        attempt: "attempt",
        evidence: {
          citations: [
            citation("https://www.lego.com/set", "Tower Bridge"),
            citation("https://shop.example/p/10", "  "),
          ],
        },
      },
    ],
    file_evidence: [
      {
        id: "file",
        node: "node",
        attempt: "attempt",
        file: { path: "/Users/reader/notes/plan.md", name: "plan.md", kind: "read" },
      },
    ],
    artifacts: [],
  } as unknown as WorkExecutionFact;
  const artifact = {
    id: "artifact",
    title: "Answer",
    data: { kind: "document", paragraphs: ["Done."] },
    evidence: [
      { extraction_id: "search", source_id: 1 },
      { extraction_id: "search", source_id: 2 },
      { extraction_id: "search", source_id: 10 },
      { extraction_id: "file", source_id: 1 },
    ],
  } as unknown as WorkArtifactV1;
  const labels = artifactView(artifact, execution).evidence.map((reference) => reference.label);
  expect(labels).toEqual(["Tower Bridge", "shop.example", "Page", "plan.md"]);
});
