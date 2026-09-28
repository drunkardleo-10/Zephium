import {
  answerCover,
  documentDigest,
  type ArtifactView,
  type DocumentNodeView,
} from "$shared/ui/data/Artifact/artifact";
import { TABLE_CARD, tableGrid } from "$shared/ui/data/Artifact/table";
import { CODE_CARD_LINES, codeLines } from "$shared/ui/data/Code";
import { objectHeight, objectWidth } from "./board/object-size";
import type { CanvasItem, CanvasSize } from "./canvas-model";

/**
 * Cards are as tall as what they say, up to a cap: the canvas sizes a node
 * before it renders, so text is measured by its average advance at the size
 * the card sets it, a little generously so the last line is never cut.
 */
const ADVANCE = { caption: 5.9, label: 6.4, strong: 6.8, body: 7.2 } as const;
const LINE = { caption: 13, label: 16, body: 17 } as const;
/** Header, body and footer of the one card frame, as `CardFrame` lays them out. */
const FRAME = { header: 20, dense: 16, footer: 29, end: 12, glyph: 24 } as const;

function textLines(text: string, width: number, advance: number, max = Infinity): number {
  const perLine = Math.max(8, Math.floor((width / advance) * 0.92));
  let count = 0;
  for (const part of text.split("\n")) {
    const length = part.trim().length;
    if (length) count += Math.ceil(length / perLine);
  }
  return Math.min(max, count);
}
const clamp = (value: number, min: number, max: number) =>
  Math.round(Math.min(max, Math.max(min, value)));

/** A card title on up to two lines beside its glyph. */
function header(title: string, width: number, dense: boolean, glyph: number = FRAME.glyph) {
  const title2 = Math.max(1, textLines(title, width - glyph - 8, ADVANCE.body, 2));
  return (dense ? FRAME.dense : FRAME.header) + Math.max(glyph, title2 * LINE.body);
}

const RESULT_CAP = 560;
/** The kind caption above a result's title. */
const KIND = 16;
/** A mini table's rows: a one-line header, then up to six rows of at most two lines. */
const TABLE_ROW = { pad: 8, header: 24 } as const;
/** A code card's line: the label size at the block's 1.6 line height. */
const CODE_LINE = 19.2;
/** The chart card's plot box. */
const CHART_PLOT = { width: 300, height: 160 } as const;

/** An answer card: 520 wide, its lead and first section at the body size, up to 360 tall. */
const ANSWER = { width: 520, cap: 360, line: 20, gap: 8, list: 6, indent: 18, fence: 16 } as const;
const plain = (node: DocumentNodeView): string =>
  node.type === "text" ? (node.text ?? "") : (node.content ?? []).map(plain).join("");
/** One block's height as `AnswerView` sets it, counted by the lines each block wraps to. */
function answerBlock(block: DocumentNodeView, width: number): number {
  switch (block.type) {
    case "heading":
      return 12 + 4 + Math.max(1, textLines(plain(block), width, ADVANCE.strong)) * LINE.label;
    case "bulletList":
    case "orderedList": {
      const items = block.content ?? [];
      return (
        items.reduce(
          (sum, item) => sum + answerBlocks(item.content ?? [], width - ANSWER.indent, 0),
          0,
        ) +
        Math.max(0, items.length - 1) * ANSWER.list
      );
    }
    case "blockquote":
      return answerBlocks(block.content ?? [], width - 12);
    case "codeBlock":
      return codeLines(plain(block)).length * CODE_LINE + ANSWER.fence;
    case "horizontalRule":
      return 1;
    default:
      return Math.max(1, textLines(plain(block), width, ADVANCE.body)) * ANSWER.line;
  }
}
function answerBlocks(
  blocks: readonly DocumentNodeView[],
  width: number,
  gap: number = ANSWER.gap,
) {
  return blocks.reduce(
    (sum, block, index) => sum + answerBlock(block, width) + (index ? gap : 0),
    0,
  );
}
/**
 * The card's size, and whether it holds less than the whole answer: then it
 * fades at its foot and the footer offers the rest. Chips need the footer too.
 */
export function answerCard(
  title: string,
  blocks: readonly DocumentNodeView[],
  { action = false, cited = false }: { action?: boolean; cited?: boolean } = {},
): { size: CanvasSize; more: boolean } {
  const inner = ANSWER.width - 24;
  const cover = answerCover(blocks);
  const top = KIND + header(title, inner, false);
  const body = answerBlocks(cover, inner);
  const more = cover.length < blocks.length || top + body + FRAME.end + 4 > ANSWER.cap;
  const foot = more || action || cited ? FRAME.footer : FRAME.end + 4;
  return {
    size: { width: ANSWER.width, height: clamp(top + body + foot, 120, ANSWER.cap) },
    more,
  };
}

/** A document is as tall as its kind caption, its whole summary and its section headings. */
export function resultSize(
  title: string,
  view: ArtifactView | undefined,
  action = false,
): CanvasSize {
  const end = action ? FRAME.footer : FRAME.end + 4;
  switch (view?.content.kind) {
    case "table":
    case "comparison": {
      const grid = tableGrid(view.content);
      const columns = Math.min(TABLE_CARD.columns, grid.columns.length);
      const width = columns >= TABLE_CARD.columns ? 420 : 360;
      const cell = (width - 24) / Math.max(1, columns) - 12;
      let body = TABLE_ROW.header;
      for (const row of grid.rows.slice(0, TABLE_CARD.rows))
        body +=
          Math.max(
            1,
            ...row.slice(0, columns).map((text) => textLines(text, cell, ADVANCE.label, 2)),
          ) *
            LINE.label +
          TABLE_ROW.pad;
      const more = grid.rows.length > TABLE_CARD.rows;
      return {
        width,
        height: clamp(
          KIND + header(title, width - 24, false) + body + (more || action ? FRAME.footer : end),
          120,
          440,
        ),
      };
    }
    case "chart":
      return {
        width: CHART_PLOT.width + 24,
        height: KIND + header(title, CHART_PLOT.width, false) + CHART_PLOT.height + end,
      };
    case "code": {
      const lines = codeLines(view.content.text).length;
      const more = lines > CODE_CARD_LINES || view.content.notes.length > 0;
      return {
        width: 420,
        height: Math.ceil(
          KIND +
            header(title, 420 - 24, false) +
            Math.min(lines, CODE_CARD_LINES) * CODE_LINE +
            (more || action ? FRAME.footer : end),
        ),
      };
    }
    case "answer":
      return answerCard(title, view.content.blocks, {
        action,
        cited: !view.knowledge && view.evidence.length > 0,
      }).size;
    case "diagram":
      return {
        width: 300,
        height: clamp(KIND + header(title, 300 - 24, false) + LINE.label + end, 96, 176),
      };
    case "matrix":
      return { width: 520, height: 320 };
    case "sources":
      return { width: 320, height: 240 };
    case "browser":
      return { width: 320, height: 180 };
    case "checklist":
      return {
        width: 420,
        height: clamp(KIND + header(title, 420 - 28, false) + LINE.label + end, 96, 176),
      };
    case "document": {
      const digest = documentDigest(view.content);
      const inner = 420 - 28;
      // The whole summary, however long, until the card reaches its cap.
      let body = textLines(digest.lead, inner, ADVANCE.label) * LINE.label;
      if (digest.headings.length) body += 10;
      for (const heading of digest.headings)
        body += Math.max(1, textLines(heading, inner - 12, ADVANCE.strong, 3)) * LINE.label + 4;
      return {
        width: 420,
        height: clamp(KIND + header(title, inner, false) + body + end, 120, RESULT_CAP),
      };
    }
    default:
      return { width: 420, height: 300 };
  }
}

/** The count, then up to six rows of mark, host and title. */
export const SOURCE_ROWS = 6;
function sourcesSize(rows: number): CanvasSize {
  const shown = Math.min(SOURCE_ROWS, rows);
  // Each row is 20 px, 4 apart, under the count.
  const body = shown * 20 + Math.max(0, shown - 1) * 4 + 4;
  return { width: 300, height: clamp(header("", 276, true) + body + FRAME.end, 96, 300) };
}

/** A fact whose label and value fit one line stands on one; the rest put the label above. */
export const factBeside = (fact: { label: string; value: string }) =>
  (fact.label.length + fact.value.length) * ADVANCE.label + 12 <= 220 - 24;
/** A subject: its picture or initial, its name on two lines, up to four whole facts, its host. */
export const SUBJECT_FACTS = 4;
const SUBJECT_TILE = 40;
function subjectSize(
  title: string,
  pictured: boolean,
  facts: readonly { label: string; value: string }[] = [],
  descriptor = "",
): CanvasSize {
  const inner = 220 - 24;
  let body = 0;
  for (const fact of facts.slice(0, SUBJECT_FACTS)) {
    body += factBeside(fact)
      ? LINE.label
      : LINE.caption + Math.max(1, textLines(fact.value, inner, ADVANCE.label, 2)) * LINE.label;
    body += 4;
  }
  if (!facts.length && descriptor)
    body += textLines(descriptor, inner, ADVANCE.label, 2) * LINE.label;
  const top = pictured
    ? 112 + header(title, 220 - 24, true, 0)
    : header(title, 220 - 24, true, SUBJECT_TILE);
  return { width: 220, height: clamp(top + body + FRAME.footer, 96, 300) };
}

/** A quiet mark and the person's words beside it, up to eight lines. */
export function requestSize(text: string, action = false): CanvasSize {
  const lines = Math.max(1, textLines(text, 300 - 24 - 24, ADVANCE.body, 8));
  // Never under 80: a checkpoint refuses a shorter placement.
  return { width: 300, height: clamp(12 + lines * LINE.body + 12 + (action ? 28 : 0), 80, 240) };
}

export function defaultSize(item: CanvasItem): CanvasSize {
  if (item.artifact) return resultSize(item.title, item.artifact, !!item.actionLabel);
  switch (item.type) {
    case "tab":
    case "link":
      return { width: 280, height: 96 };
    case "subject":
      return subjectSize(
        item.title,
        !!item.image || !!item.subject?.imageCandidates?.length,
        item.facts,
        item.detail,
      );
    case "sources":
      return sourcesSize(item.sources?.length ?? 0);
    case "folder":
      return { width: 248, height: 96 };
    case "note":
      return { width: 300, height: 200 };
    case "media":
      return { width: 248, height: 200 };
    case "objective":
    case "request":
      return requestSize(item.title, !!item.actionLabel);
    case "responsibility":
      return { width: 280, height: 150 };
    case "page":
      return { width: 248, height: 168 };
    case "agent":
      return { width: 24, height: 24 };
    case "object": {
      const view = item.object?.view;
      if (!view) return { width: 280, height: 160 };
      const width = objectWidth(view).ideal;
      return { width, height: Math.round(objectHeight(view, width)) };
    }
    default:
      return { width: 280, height: 160 };
  }
}
