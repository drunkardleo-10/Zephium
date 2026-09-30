import type { BoardLayout, Rect } from "../board/layout";
import { branch, merge, type Point } from "./lines";

/**
 * A run on the 8 px grid, left to right: the inputs hang left of the request
 * at x = 0, the parts start 96 past the request, the result 96 past the
 * widest part row. The parts stand as a column centred on the request, so
 * parallel work forks from one point, up and down alike, and merges the same
 * way into the result. The run's spine runs through the request's first line,
 * the middle of the fork and the answer's headline. Its Sources stand under
 * its result.
 */
export const RUN = {
  inputs: 200,
  request: 320,
  gutter: 48,
  /** From the request to its parts: the fork's trunk stands halfway. */
  fork: 96,
  /** Between part rows: room for each helper's line over its part. */
  rowGap: 48,
  between: 120,
  /** From the widest part row to the result: room for the lines to merge. */
  feed: 96,
  found: 32,
  foundGap: 24,
  /** A row's finds wrap past this width, so the result stays in view. */
  foundRow: 1080,
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
  kind: "part" | "feed" | "input" | "found";
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
  const lines: RunLine[] = [];
  const partsX = RUN.request + RUN.fork;
  let y = top + RUN.spine - RUN.labelMid;
  let widest = 0;
  const rowEnds: { row: RunRow; end: Point; last: string; lastRect: Rect }[] = [];
  for (const row of run.rows) {
    const rowTop = y;
    rects[row.part.id] = { x: partsX, y: rowTop, width: row.part.width, height: row.part.height };
    const start = snap(partsX + row.part.width + RUN.found);
    let x = start;
    let lineTop = rowTop;
    let lineHeight = 0;
    let height = row.part.height;
    let last = row.part.id;
    let farthest = partsX + row.part.width;
    // What a part found runs along its row, and wraps under itself rather than pushing the result off the view.
    for (const found of row.found) {
      if (x > start && x + found.width - start > RUN.foundRow) {
        lineTop = snap(lineTop + lineHeight + RUN.foundGap);
        x = start;
        lineHeight = 0;
      }
      rects[found.id] = { x, y: lineTop, width: found.width, height: found.height };
      if (lineTop === rowTop) last = found.id;
      farthest = Math.max(farthest, x + found.width);
      x = snap(x + found.width + RUN.foundGap);
      lineHeight = Math.max(lineHeight, found.height);
      height = Math.max(height, lineTop - rowTop + found.height);
    }
    const first = row.found[0] ? rects[row.found[0].id] : undefined;
    // The part's work is joined to what it found, straight along the row.
    if (first) {
      const y = rowTop + RUN.labelMid;
      const from = { x: partsX + row.part.width + RUN.air, y };
      lines.push({
        id: `found:${row.part.id}`,
        kind: "found",
        source: row.part.id,
        target: row.found[0]!.id,
        points: [from, { x: first.x - RUN.air, y }],
        from: { x: row.part.width + RUN.air, y: RUN.labelMid },
        to: { x: -RUN.air, y: RUN.labelMid },
        laid: ORIGIN,
      });
    }
    const lastRect = rects[last]!;
    const right = lastRect.x + lastRect.width;
    widest = Math.max(widest, farthest - partsX);
    rowEnds.push({ row, end: { x: right + RUN.air, y: rowTop + RUN.labelMid }, last, lastRect });
    y = snap(rowTop + height + RUN.rowGap);
  }
  const partsBottom = run.rows.length ? y - RUN.rowGap : top;
  const labels = run.rows.map((row) => rects[row.part.id]!.y + RUN.labelMid);
  const spine = labels.length
    ? Math.max(top + RUN.spine, Math.round((labels[0]! + labels.at(-1)!) / 8) * 4)
    : top + RUN.spine;
  const request = { x: 0, y: spine - RUN.spine, width: RUN.request, height: run.request.height };
  rects[run.request.id] = request;

  let inputsBottom = top;
  const inputs = run.inputs ?? [];
  if (inputs.length) {
    const x = -(RUN.inputs + RUN.gutter);
    const tall = inputs.reduce((sum, input) => sum + input.height, 0) + (inputs.length - 1) * 8;
    let y = Math.max(top, spine - tall / 2);
    const ends: Point[] = [];
    for (const input of inputs) {
      rects[input.id] = { x, y, width: input.width, height: input.height };
      ends.push({ x: x + input.width + RUN.air, y: y + input.height / 2 });
      y += input.height + 8;
    }
    inputsBottom = y;
    const into = { x: -RUN.air, y: spine };
    merge(ends, -RUN.gutter / 2, into).forEach((points, index) => {
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

  const resultX = run.rows.length ? snap(partsX + widest + RUN.feed) : RUN.request + RUN.gutter;
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

  // A request's line branches to the head of every row; each row's end merges into the result.
  const start = { x: RUN.request + RUN.air, y: spine };
  const partEnds = labels.map((y) => ({ x: partsX - RUN.air, y }));
  branch(start, RUN.request + RUN.fork / 2, partEnds).forEach((points, index) => {
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
    const ends = feeding.map((entry) => entry.end);
    merge(ends, resultX - RUN.feed / 2, into).forEach((points, index) => {
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
    request.y + run.request.height,
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
