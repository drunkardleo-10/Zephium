import { codeLines } from "$shared/ui/data/Code/code";
import { documentDigest } from "$shared/ui/data/Artifact/artifact";
import { diagramLayout } from "../diagram";
import { plain } from "./text";
import type { Block, ColumnType } from "./types";

/**
 * How wide a block may stand and how tall it is at a width, before it has
 * measured itself: the first frame's guess, corrected by the block's own
 * size once it has drawn. The frame every block shares is `PAD` around its
 * title and body.
 */
export const PAD = 20;
const TITLE = 20 + 12;
const chrome = (block: Block) => PAD * 2 + (block.title && block.kind !== "prose" ? TITLE : 0);
const lines = (text: string, width: number, advance: number) =>
  text
    .split("\n")
    .reduce(
      (sum, part) =>
        sum + Math.max(part.trim() ? 1 : 0, Math.ceil(part.length / Math.max(8, width / advance))),
      0,
    );

/** A gallery shows this many before it counts the rest. */
export const GALLERY = 6;
/** Rows a table shows before it opens; a gallery's card; a code block's lines. */
export const TABLE_ROWS = 8;
export const CODE_LINES = 16;
export const CARD = { width: 216, gap: 12 } as const;
/** A card with a picture is as wide as its picture; one with a mark beside its words, wider and fewer across. */
export const cardWidth = (block: { entities: readonly { image?: unknown }[] }) =>
  block.entities.some((entity) => entity.image) ? CARD.width : 300;
const MARKED_ACROSS = 3;
const COLUMN: Record<ColumnType, number> = {
  text: 150,
  long: 300,
  number: 110,
  money: 120,
  date: 130,
  link: 170,
};

export function widthRange(block: Block): { min: number; ideal: number; max: number } {
  switch (block.kind) {
    case "prose":
      return { min: 420, ideal: 620, max: 660 };
    case "entity":
      return { min: 260, ideal: 300, max: 340 };
    case "gallery": {
      const card = cardWidth(block);
      const shown = Math.min(block.entities.length, card === CARD.width ? GALLERY : MARKED_ACROSS);
      const across = (count: number) => count * card + (count - 1) * CARD.gap + PAD * 2;
      // Cards with a mark read one under another when the room is narrow.
      return { min: across(card === CARD.width ? 2 : 1), ideal: across(shown), max: 1280 };
    }
    case "comparison": {
      const count =
        block.content.kind === "matrix"
          ? block.content.subjects.length
          : block.content.criteria.length;
      const ideal = 200 + count * 180 + PAD * 2;
      return { min: Math.min(ideal, 520), ideal, max: 1280 };
    }
    case "table": {
      const ideal = block.columns.reduce((sum, column) => sum + COLUMN[column.type], PAD * 2);
      // A table with a long column reads better wide; one of figures stays tight.
      const long = block.columns.some((column) => column.type === "long");
      return { min: Math.min(ideal, 360), ideal, max: Math.max(ideal + (long ? 360 : 80), 420) };
    }
    case "chart":
      return block.values ? { min: 420, ideal: 560, max: 720 } : { min: 340, ideal: 480, max: 640 };
    case "timeline":
      return { min: 480, ideal: Math.max(640, block.stops.length * 200), max: 1280 };
    case "diagram": {
      const layout = diagramLayout(block.diagram);
      const ideal = Math.ceil(layout.bounds.width) + PAD * 2;
      return { min: Math.min(ideal, 720), ideal, max: 1600 };
    }
    case "checklist":
      return { min: 300, ideal: 380, max: 460 };
    case "code":
      return { min: 460, ideal: 600, max: 720 };
    case "document":
      return { min: 340, ideal: 440, max: 560 };
    case "stat":
      return { min: 180, ideal: 220, max: 280 };
    case "callout":
      return { min: 300, ideal: 420, max: 560 };
  }
}

/** A block's height at a width, closed or opened in place. */
export function estimate(block: Block, width: number, open = false): number {
  const inner = width - PAD * 2;
  const frame = chrome(block);
  switch (block.kind) {
    case "prose": {
      if (block.state === "pending") return 132;
      let body = 0;
      for (const node of block.blocks) {
        const text = plain(node);
        body +=
          node.type === "heading"
            ? 36
            : node.type === "codeBlock"
              ? codeLines(text).length * 19 + 24
              : lines(text, inner, 7.6) * 22 + 12;
      }
      return frame + body + 8;
    }
    case "entity":
      return (
        (block.entity.image ? Math.round(width * 0.56) : 0) + 116 + block.entity.facts.length * 22
      );
    case "gallery": {
      const shown = open ? block.entities.length : Math.min(block.entities.length, GALLERY);
      const across = Math.max(1, Math.floor((inner + CARD.gap) / (cardWidth(block) + CARD.gap)));
      const rows = Math.ceil(shown / across);
      const card = block.entities.some((entity) => entity.image) ? 136 + 150 : 120;
      const footer = block.compare || block.entities.length > GALLERY ? 42 : 0;
      const compare = open && block.compare ? 60 + block.compare.criteria.length * 44 : 0;
      return frame + rows * card + (rows - 1) * 20 + footer + compare;
    }
    case "comparison": {
      const rows =
        block.content.kind === "matrix"
          ? block.content.criteria.length
          : block.content.alternatives.length;
      const head = block.content.kind === "matrix" ? 120 : 36;
      return frame + head + (open ? rows : Math.min(rows, 6)) * 36 + 36;
    }
    case "table": {
      const rows = open ? block.rows.length : Math.min(block.rows.length, TABLE_ROWS);
      return frame + 36 + rows * 38 + (block.rows.length > TABLE_ROWS ? 40 : 0);
    }
    case "chart":
      return (
        frame +
        (block.headline ? 52 : 0) +
        260 +
        (block.values ? 40 + (open ? 60 + block.values.rows.length * 38 : 0) : 0)
      );
    case "timeline":
      return frame + (block.stops.length <= 6 && width >= 640 ? 150 : block.stops.length * 56);
    case "diagram":
      return frame + Math.ceil(diagramLayout(block.diagram).bounds.height) + 28;
    case "checklist":
      return frame + block.items.length * 32 + 48;
    case "code": {
      const count = codeLines(block.text).length;
      return (
        frame +
        (open ? count : Math.min(count, CODE_LINES)) * 20 +
        24 +
        (count > CODE_LINES || block.notes.length ? 40 : 0)
      );
    }
    case "document": {
      if (open)
        return (
          frame +
          block.content.paragraphs.reduce((sum, text) => sum + lines(text, inner, 7.4) * 21 + 10, 0)
        );
      const digest = documentDigest(block.content);
      return frame + lines(digest.lead, inner, 7.4) * 21 + digest.headings.length * 24 + 48;
    }
    case "stat":
      return 112;
    case "callout":
      return frame + lines(block.text, inner, 7.4) * 21;
  }
}
