import { expect, test } from "vitest";
import type { WorkExecutionFact } from "$shared/ipc/bindings";
import { runSources } from "../lib/run/sources";
import { RUN, placeRun } from "../lib/run/layout";
import { PART, partSize, stackSize } from "../lib/run/part-size";
import { projection } from "./environment-fixtures";

function run(patch: Partial<WorkExecutionFact>): WorkExecutionFact {
  return { ...structuredClone(projection.executions[0]!), artifacts: [], steps: [], ...patch };
}
const citation = (url: string, title: string) => ({ url, title, start_index: 0, end_index: 1 });
const searched = (id: string, citations: ReturnType<typeof citation>[]) =>
  ({
    id,
    node: "node",
    attempt: "attempt",
    evidence: { version: 1, provider: "open_ai", model: "m", answer: "", citations },
  }) as unknown as NonNullable<WorkExecutionFact["provider_evidence"]>[number];

test("Sources lead with what the objects cite, then what was read, each page once", () => {
  const execution = run({
    provider_evidence: [
      searched("ex1", [
        citation("https://aws.amazon.com/lambda/pricing/", "Lambda pricing"),
        citation("https://vercel.com/pricing", "Vercel pricing"),
      ]),
    ],
    steps: [
      {
        id: "s1",
        turn: 1,
        kind: { kind: "search", query: "q" },
        status: "succeeded",
        evidence: "ex1",
      },
      {
        id: "r1",
        turn: 1,
        kind: { kind: "read", url: "http://vercel.com/pricing" },
        status: "succeeded",
        local: { page_title: "Vercel pricing" },
      },
      {
        id: "r2",
        turn: 1,
        kind: { kind: "read", url: "https://docs.hetzner.com/cloud/" },
        status: "failed",
        note: "The browser was not ready for this page",
      },
    ] as WorkExecutionFact["steps"],
    artifacts: [
      {
        ...structuredClone(projection.executions[0]!.artifacts[0]!),
        evidence: [{ extraction_id: "ex1", source_id: 2 }],
      },
    ],
  });
  const sources = runSources([execution]);
  expect(sources.rows.map((row) => row.title)).toEqual(["Vercel pricing", "Lambda pricing"]);
  expect(sources.rows[0]!.host).toBe("vercel.com");
  expect(sources.unread).toEqual([
    expect.objectContaining({
      host: "docs.hetzner.com",
      note: "The browser was not ready for this page",
    }),
  ]);
});

test("a part's pages stand as one stack of windows, the same while it works and once it is done", () => {
  const stack = stackSize(3);
  expect(stack).toEqual({ width: PART.window + 2 * PART.behindX, height: 238 + 2 * PART.behindY });
  expect(partSize({ kind: "pages", count: 3 }).height).toBe(stack.height);
  // A name that runs to more lines than its row makes the row taller, never overlapping the next.
  expect(partSize({ kind: "label" }, 120).height).toBe(120);
});

test("a run's Sources stand under its result, and the result waits past a gutter for the lines", () => {
  const place = placeRun(0, {
    request: { id: "request", width: RUN.request, height: 80 },
    rows: [{ part: { id: "part", width: 504, height: 258 }, found: [], feeds: true }],
    head: { id: "head", width: 640, height: 120 },
    board: { width: 640, height: 0, at: {} },
    sources: { id: "sources", width: 560, height: 200 },
  });
  const part = place.rects["part"]!;
  expect(place.corner.x - (part.x + part.width)).toBeGreaterThanOrEqual(RUN.feed);
  const sources = place.rects["sources"]!;
  expect(sources.x).toBe(place.corner.x);
  expect(sources.y).toBeGreaterThanOrEqual(place.rects["head"]!.y + 120 + RUN.sources);
  expect(place.extent).toBe(sources.y + sources.height);
});
