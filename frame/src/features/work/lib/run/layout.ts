import type { BoardLayout, Rect } from "../board/layout";
import { fanIn, fanOut, type Point } from "./lines";

/**
 * A run on the 8 px grid, left to right: the inputs hang left of the request
 * at x = 0, the parts start 48 past the request, the result 96 past the
 * widest part row, so the lines from what the parts found merge in a gutter
 * of their own. Every line of a run meets its ends on one spine, 32 under
 * the run's top: the request's first line, each part's name, the answer's
 * headline. The run's Sources stand under its result.
 */
export const RUN = {
  inputs: 200,
  request: 320,
  gutter: 48,
  rowGap: 32,
  between: 120,
  /** From the widest part row to the result: room for the lines to merge. */
  feed: 96,
  found: 32,
  foundGap: 24,
  /** The Sources under the result. */
  sources: 40,
  spine: 32,
  /** A part's name sits 12 under its row's top. */
  labelMid: 12,
  /** The answer's headline centre, under the result's top. */
  resultMid: 14,
  /** Lines stop short of what they join. */
  air: 6,
  head: 16,
} as const;

const snap = (value: number) => Math.ceil(value / 8) * 8;

type Sized = { id: string; width: number; height: number };
type RunRow = {
  part: Sized;
  /** What the part found, in the order it found it, at the end of its row. */
  found: readonly Sized[];
  /** The part's work reaches the result. */
  feeds: boolean;
};
export type RunInputs = {
  request: Sized;
  inputs?: readonly Sized[];
  rows: readonly RunRow[];
  head?: Sized;
  board: BoardLayout;
  /** What the run drew on, under its result. */
  sources?: Sized;
  /** Blocks the person moved, by their offset from the result's corner. */
  pins?: ReadonlyMap<string, Rect>;
};
type RunLine = {
  id: string;
  kind: "part" | "feed" | "input";
  source: string;
  target: string;
  /** The route as laid out, in canvas coordinates. */
  points: Point[];
  /** Where the line truly leaves and lands, relative to its two nodes' corners. */
  from: Point;
  to: Point;
  /** Where the run put its two nodes' corners. */
  laid: { source: Point; target: Point };
};
export type RunPlace = {
  rects: Record<string, Rect>;
  /** The result's blocks, under its head. */
  board: Rect;
  /** The result's top-left: a moved block is kept relative to it. */
  corner: Point;
  /** The bottom of whatever the run holds; the next run starts 120 under it. */
  extent: number;
  /** Everything the run covers, for keeping the person's own things clear of it. */
  box: Rect;
  lines: RunLine[];
};

const ORIGIN = { source: { x: 0, y: 0 }, target: { x: 0, y: 0 } };

export function placeRun(top: number, run: RunInputs): RunPlace {
  const rects: Record<string, Rect> = {};
  const spine = top + RUN.spine;
  const request = { x: 0, y: top, width: RUN.request, height: run.request.height };
  rects[run.request.id] = request;
  const lines: RunLine[] = [];

  let inputsBottom = top;
  const inputs = run.inputs ?? [];
  if (inputs.length) {
    const x = -(RUN.inputs + RUN.gutter);
    let y = spine - (inputs[0]?.height ?? 0) / 2;
    const ends: Point[] = [];
    for (const input of inputs) {
      rects[input.id] = { x, y, width: input.width, height: input.height };
      ends.push({ x: x + input.width + RUN.air, y: y + input.height / 2 });
      y += input.height + 8;
    }
    inputsBottom = y;
    const into = { x: -RUN.air, y: spine };
    fanIn(ends, -RUN.gutter / 2, into).forEach((points, index) => {
      const input = inputs[index]!;
      const rect = rects[input.id]!;
      lines.push({
        id: `input:${input.id}`,
        kind: "input",
        source: input.id,
        target: run.request.id,
        points,
        from: { x: rect.width + RUN.air, y: rect.height / 2 },
        to: { x: -RUN.air, y: RUN.spine },
        laid: ORIGIN,
      });
    });
  }

  const partsX = RUN.request + RUN.gutter;
  let y = spine - RUN.labelMid;
  let widest = 0;
  const rowEnds: { row: RunRow; end: Point; last: string; lastRect: Rect }[] = [];
  for (const row of run.rows) {
    const rowTop = y;
    rects[row.part.id] = { x: partsX, y: rowTop, width: row.part.width, height: row.part.height };
    let x = snap(partsX + row.part.width + RUN.found);
    let height = row.part.height;
    let last = row.part.id;
    for (const found of row.found) {
      rects[found.id] = { x, y: rowTop, width: found.width, height: found.height };
      last = found.id;
      x = snap(x + found.width + RUN.foundGap);
      height = Math.max(height, found.height);
    }
    const lastRect = rects[last]!;
    const right = lastRect.x + lastRect.width;
    widest = Math.max(widest, right - partsX);
    rowEnds.push({ row, end: { x: right + RUN.air, y: rowTop + RUN.labelMid }, last, lastRect });
    y = snap(rowTop + height + RUN.rowGap);
  }
  const partsBottom = run.rows.length ? y - RUN.rowGap : top;

  const resultX = run.rows.length ? snap(partsX + widest + RUN.feed) : partsX;
  const resultTop = spine - RUN.resultMid;
  let blocks = resultTop;
  if (run.head) {
    rects[run.head.id] = {
      x: resultX,
      y: resultTop,
      width: run.head.width,
      height: run.head.height,
    };
    blocks = resultTop + run.head.height + RUN.head;
  }
  for (const [id, rect] of Object.entries(run.board.at))
    rects[id] = { ...rect, x: rect.x + resultX, y: rect.y + blocks };
  const resultBottom = blocks + run.board.height - (run.head && !run.board.height ? RUN.head : 0);
  const resultId = run.head?.id ?? Object.keys(run.board.at)[0];
  let resultRight = resultX + Math.max(run.head?.width ?? 0, run.board.width);

  // A request's line reaches the head of every row; its row's end reaches the result.
  const start = { x: RUN.request + RUN.air, y: spine };
  const trunk = RUN.request + RUN.gutter / 2;
  const partEnds = run.rows.map((row) => ({
    x: partsX - RUN.air,
    y: rects[row.part.id]!.y + RUN.labelMid,
  }));
  fanOut(start, trunk, partEnds).forEach((points, index) => {
    const row = run.rows[index]!;
    lines.push({
      id: `line:${row.part.id}`,
      kind: "part",
      source: run.request.id,
      target: row.part.id,
      points,
      from: { x: RUN.request + RUN.air, y: RUN.spine },
      to: { x: -RUN.air, y: RUN.labelMid },
      laid: ORIGIN,
    });
  });
  if (resultId) {
    const into = { x: resultX - RUN.air, y: spine };
    const result = rects[resultId]!;
    const to = { x: -RUN.air, y: spine - result.y };
    const feeding = rowEnds.filter((entry) => entry.row.feeds);
    fanIn(
      feeding.map((entry) => entry.end),
      resultX - RUN.feed / 2,
      into,
    ).forEach((points, index) => {
      const entry = feeding[index]!;
      lines.push({
        id: `feed:${entry.row.part.id}`,
        kind: "feed",
        source: entry.last,
        target: resultId,
        points,
        from: {
          x: entry.lastRect.width + RUN.air,
          y: entry.end.y - entry.lastRect.y,
        },
        to,
        laid: ORIGIN,
      });
    });
    if (!run.rows.length)
      lines.push({
        id: `line:${run.request.id}:${resultId}`,
        kind: "part",
        source: run.request.id,
        target: resultId,
        points: [start, into],
        from: { x: RUN.request + RUN.air, y: RUN.spine },
        to,
        laid: ORIGIN,
      });
  }

  let sourcesBottom = resultBottom;
  if (run.sources) {
    // Under the result; a run that came to no result keeps them under its parts.
    const bare = !resultId && run.rows.length;
    const x = bare ? partsX : resultX;
    const y = snap((bare ? partsBottom : resultBottom) + RUN.sources);
    rects[run.sources.id] = { x, y, width: run.sources.width, height: run.sources.height };
    sourcesBottom = y + run.sources.height;
    resultRight = Math.max(resultRight, x + run.sources.width);
  }
  let extent = Math.max(
    top + run.request.height,
    inputsBottom,
    partsBottom,
    resultBottom,
    sourcesBottom,
  );
  for (const [id, pin] of run.pins ?? []) {
    rects[id] = { ...pin, x: resultX + pin.x, y: resultTop + pin.y };
    extent = Math.max(extent, resultTop + pin.y + pin.height);
    resultRight = Math.max(resultRight, resultX + pin.x + pin.width);
  }
  for (const line of lines) {
    const a = rects[line.source]!;
    const b = rects[line.target]!;
    line.laid = { source: { x: a.x, y: a.y }, target: { x: b.x, y: b.y } };
  }
  const left = inputs.length ? -(RUN.inputs + RUN.gutter) : 0;
  return {
    rects,
    board: { x: resultX, y: blocks, width: run.board.width, height: run.board.height },
    corner: { x: resultX, y: resultTop },
    extent,
    box: { x: left, y: top, width: resultRight - left, height: extent - top },
    lines,
  };
}
