import { expect, test } from "vitest";
import { edgeClass, type CanvasLink } from "../lib/canvas-model";
import { environmentClusters } from "../lib/project-environment";
import type { WorkStage } from "../lib/project-environment-thread";
import { SIZES, stageLayout, type StageContents } from "../lib/stage-layout";

const one = (id: string, size: { width: number; height: number }) => ({
  members: [{ id, size }],
});

function stage(contents: StageContents): WorkStage {
  const place = { x: 0, y: 0, ...SIZES.request };
  return {
    element: "request",
    objective: "objective",
    card: "request",
    request: "Compare quiet keyboards",
    executions: [],
    place,
    contents,
    layout: stageLayout(place, contents),
  };
}

test("the path reads through the stage at rest; relations wait for a focused end", () => {
  const { clusters, links } = environmentClusters([
    stage({
      sources: one("sources", SIZES.sources),
      pages: {
        members: [...one("page-a", SIZES.page).members, ...one("page-b", SIZES.page).members],
      },
      subjects: one("subject", SIZES.subject),
      findings: one("findings", SIZES.findings),
      results: one("result", SIZES.document),
    }),
  ]);
  // Pages and subjects gather under a label even alone; single cards stand for themselves.
  expect(clusters.map((cluster) => [cluster.id, cluster.label, cluster.members])).toEqual([
    ["cluster:request:pages", "2 pages", ["page-a", "page-b"]],
    ["cluster:request:subjects", "1 subject", ["subject"]],
  ]);
  expect(links.map((link) => [link.kind, link.source, link.target])).toEqual([
    ["path", "request", "sources"],
    ["path", "sources", "cluster:request:pages"],
    ["path", "cluster:request:pages", "cluster:request:subjects"],
    ["path", "cluster:request:subjects", "findings"],
    ["path", "findings", "result"],
  ]);
  const path = links[0]!;
  const relation: CanvasLink = {
    id: "relation:1",
    source: "page-a",
    target: "subject",
    kind: "supports",
    label: "supports",
  };
  const rest = new Set<string>();
  expect(edgeClass(path, rest, false)).toBe("work-edge kind-path");
  expect(edgeClass(path, rest, true)).toBe("work-edge kind-path draw");
  expect(edgeClass(relation, rest, true)).toBe("work-edge kind-supports latent");
  expect(edgeClass(relation, new Set(["subject"]), false)).toBe(
    "work-edge kind-supports latent active",
  );
  expect(edgeClass({ ...relation, kind: "working" }, rest, false)).toBe("work-edge kind-working");
});

test("an empty cluster is skipped: the path goes to the next stop", () => {
  const { links } = environmentClusters([
    stage({ sources: one("sources", SIZES.sources), results: one("result", SIZES.result) }),
  ]);
  expect(links.map((link) => [link.source, link.target])).toEqual([
    ["request", "sources"],
    ["sources", "result"],
  ]);
});
