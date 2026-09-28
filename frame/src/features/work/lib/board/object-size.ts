import { diagramLayout, diagramWidth } from "../diagram";
import type { ObjectView, SheetColumn } from "./types";

type Range = { min: number; ideal: number; max: number };

/** A pick's card at its natural width; a set shows up to four across. */
const CARD = { pictured: 232, plain: 272, gap: 16, across: 4 } as const;
const COLUMN: Record<SheetColumn["kind"], number> = {
  entity: 208,
  text: 168,
  number: 104,
  money: 112,
  percent: 96,
  date: 120,
  duration: 104,
  yes_no: 88,
  rating: 104,
  link: 152,
  tag: 112,
};

const lines = (text: string, width: number, advance: number) =>
  Math.max(1, Math.ceil((text.length * advance) / Math.max(120, width)));

/** How wide an object may stand: its least, its natural and its widest. */
export function objectWidth(view: ObjectView): Range {
  switch (view.kind) {
    case "reply":
      return { min: 480, ideal: 640, max: 680 };
    case "picks": {
      if (view.facet === "flight" && view.items.every((item) => item.route))
        return { min: 440, ideal: 560, max: 640 };
      const card = view.items.some((item) => item.picture) ? CARD.pictured : CARD.plain;
      const across = Math.min(CARD.across, Math.max(1, view.items.length));
      const width = (count: number) => count * card + (count - 1) * CARD.gap;
      return { min: width(Math.min(2, across)), ideal: width(across), max: width(across) + 160 };
    }
    case "plan":
      return { min: 440, ideal: 640, max: 760 };
    case "list":
      return { min: 400, ideal: 560, max: 640 };
    case "sheet": {
      const ideal = view.columns.reduce((sum, column) => sum + COLUMN[column.kind], 40);
      return { min: Math.min(ideal, 480), ideal: Math.min(ideal, 1120), max: 1120 };
    }
    case "plot":
      return { min: 360, ideal: 520, max: 680 };
    case "diff":
      return { min: 520, ideal: 680, max: 860 };
    case "draft":
      return { min: 420, ideal: 520, max: 600 };
    case "media":
      return view.media === "audio"
        ? { min: 320, ideal: 400, max: 480 }
        : { min: 480, ideal: 560, max: 720 };
    case "page":
      return { min: 320, ideal: 400, max: 480 };
    case "note":
      return { min: 280, ideal: 360, max: 480 };
    case "file":
      return { min: 240, ideal: 280, max: 360 };
    case "folder":
      return { min: 240, ideal: 280, max: 320 };
    case "code":
      return { min: 460, ideal: 600, max: 720 };
    case "document":
      return { min: 360, ideal: 480, max: 560 };
    case "diagram": {
      // A diagram stands at its own drawn width, so its parts read at their size.
      const ideal = Math.min(1600, diagramWidth(view.diagram) + 40);
      return { min: Math.min(ideal, 720), ideal, max: 1600 };
    }
  }
}

/** An object's height at a width before it has measured itself. */
export function objectHeight(view: ObjectView, width: number): number {
  switch (view.kind) {
    case "reply":
      return (
        lines(view.headline, width, 14) * 31 +
        (view.text ? 14 + lines(view.text, width, 8) * 23 : 0) +
        (view.figures.length ? 14 + 56 : 0) +
        (view.points.length ? 14 + view.points.length * 30 : 0)
      );
    case "picks": {
      if (view.facet === "flight" && view.items.every((item) => item.route))
        return 44 + view.items.length * 108;
      const pictured = view.items.some((item) => item.picture);
      const card = pictured ? CARD.pictured : CARD.plain;
      const across = Math.max(1, Math.floor((width + CARD.gap) / (card + CARD.gap)));
      const rows = Math.ceil(view.items.length / across);
      const height = pictured ? Math.round(card * 0.75) + 132 : 148;
      return (view.title ? 34 : 0) + rows * height + (rows - 1) * CARD.gap;
    }
    case "plan":
      return (view.title ? 40 : 0) + view.steps.length * 56 + (view.total ? 48 : 0) + 16;
    case "list":
      return (
        (view.title ? 40 : 0) +
        view.items.reduce((sum, item) => sum + (item.from ? 76 : item.detail ? 56 : 40), 0)
      );
    case "sheet": {
      const rows = Math.min(view.rows.length, 8);
      return (view.title ? 40 : 0) + 44 + rows * 44 + (view.rows.length > 8 ? 40 : 0);
    }
    case "plot":
      return (view.title ? 40 : 0) + (view.spec.headline ? 56 : 0) + 300;
    case "diff":
      return (
        72 +
        Math.min(
          40,
          view.hunks.reduce((sum, hunk) => sum + hunk.lines.length, 0),
        ) *
          20
      );
    case "draft":
      return 120 + lines(view.body, width, 8) * 22;
    case "media":
      return view.media === "audio" ? 72 : Math.round(width * 0.5625) + 40;
    case "page":
      return Math.round(width * 0.625) + 48;
    case "note":
      return 48 + lines(view.markdown, width, 8) * 22;
    case "file":
      return 220;
    case "folder":
      return 180;
    case "code":
      return 80 + Math.min(16, view.text.split("\n").length) * 20;
    case "document":
      return 320;
    case "diagram":
      return Math.ceil(diagramLayout(view.diagram).bounds.height) + 80;
  }
}
