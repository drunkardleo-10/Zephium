import { documentDigest, type ArtifactView } from "$shared/ui/data/Artifact/artifact";
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
/** A document is as tall as its summary (whole, up to 14 lines) and its section headings. */
export function resultSize(
  title: string,
  view: ArtifactView | undefined,
  action = false,
): CanvasSize {
  const end = action ? FRAME.footer : FRAME.end + 4;
  switch (view?.content.kind) {
    case "comparison":
    case "matrix":
      return { width: 520, height: 320 };
    case "sources":
      return { width: 320, height: 240 };
    case "browser":
      return { width: 320, height: 180 };
    case "checklist":
      return {
        width: 420,
        height: clamp(header(title, 420 - 28, false) + LINE.label + end, 96, 160),
      };
    case "document": {
      const digest = documentDigest(view.content);
      const inner = 420 - 28;
      let body = textLines(digest.lead, inner, ADVANCE.label, 14) * LINE.label;
      if (digest.headings.length) body += 10;
      for (const heading of digest.headings)
        body += Math.max(1, textLines(heading, inner - 12, ADVANCE.strong, 3)) * LINE.label + 4;
      return {
        width: 420,
        height: clamp(header(title, inner, false) + body + end, 120, RESULT_CAP),
      };
    }
    default:
      return { width: 420, height: 300 };
  }
}

/** Up to eight claims of two lines each, then "+n" when there are more. */
export const FINDINGS_ROWS = 8;
export function findingsSize(
  claims: readonly { claim: string; subject?: string }[],
  total = claims.length,
): CanvasSize {
  let body = 0;
  for (const claim of claims.slice(0, FINDINGS_ROWS))
    body +=
      Math.max(
        1,
        textLines(claim.claim, 300 - 24 - 14 - (claim.subject ? 96 : 0), ADVANCE.label, 2),
      ) *
        LINE.label +
      3;
  if (total > FINDINGS_ROWS) body += LINE.caption + 3;
  return { width: 300, height: clamp(header("", 276, true) + body + 6 + FRAME.end, 96, 420) };
}

/** A favicon row and up to six host · title rows. */
export const SOURCE_ROWS = 6;
export function sourcesSize(rows: number): CanvasSize {
  const shown = Math.min(SOURCE_ROWS, rows);
  const body = (rows ? 28 : 0) + shown * LINE.label + Math.max(0, shown - 1) * 6 + 4;
  return { width: 300, height: clamp(header("", 276, true) + body + FRAME.end, 96, 300) };
}

/** A fact whose label and value fit one line stands on one; the rest put the label above. */
export const factBeside = (fact: { label: string; value: string }) =>
  (fact.label.length + fact.value.length) * ADVANCE.label + 12 <= 220 - 24;
/** A subject: its picture or initial, its name on two lines, up to four whole facts, its host. */
export const SUBJECT_FACTS = 4;
const SUBJECT_TILE = 40;
export function subjectSize(
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

/** A plan step: its glyph and number, then the whole step and its first line of detail. */
export function stepSize(text: string, detail = ""): CanvasSize {
  const inner = 248 - 28;
  const body =
    Math.max(1, textLines(text, inner, ADVANCE.strong, 8)) * LINE.label +
    (detail ? textLines(detail, inner, ADVANCE.caption, 4) * 15 + 4 : 0);
  return { width: 248, height: clamp(14 + 28 + 8 + body + 14, 96, 240) };
}

/** The person's words, up to five lines. */
export function requestSize(text: string, action = false): CanvasSize {
  const lines = Math.max(1, textLines(text, 300 - 32 - 34, ADVANCE.body, 5));
  return { width: 300, height: clamp(31 + lines * LINE.body + (action ? 28 : 0), 64, 180) };
}

export function defaultSize(item: CanvasItem): CanvasSize {
  if (item.type === "findings")
    return findingsSize(item.findings?.items ?? [], item.findings?.total);
  if (item.type === "step") return stepSize(item.step?.text ?? item.title, item.detail);
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
    case "finding":
      return { width: 300, height: 140 };
    case "sources":
      return sourcesSize(item.sources?.length ?? 0);
    case "folder":
    case "file":
      return { width: 248, height: 96 };
    case "command":
      return { width: 248, height: 120 };
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
    default:
      return { width: 280, height: 160 };
  }
}
