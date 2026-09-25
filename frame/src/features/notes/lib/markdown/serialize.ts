import type { JSONContent } from "@tiptap/core";
import { Fragment, type Mark, type Node } from "@tiptap/pm/model";
import { ENTITY } from "./entities";
import { parse } from "./parse";

/** How a line of text is written. The first attempt escapes as little as
 *  possible and keeps the delimiters it was read with; later attempts, used
 *  only when an earlier one would not read back as the same content, trade
 *  that for certainty. `emphasis: "none"` gives up emphasis Markdown cannot
 *  express rather than let its delimiters become text. */
type Escaping = {
  escape: "minimal" | "strict";
  emphasis: "kept" | "alternate" | "none";
};
const ATTEMPTS: Escaping[] = [
  { escape: "minimal", emphasis: "kept" },
  // Italic against bold (`***`) is ambiguous; different characters are not.
  { escape: "minimal", emphasis: "alternate" },
  { escape: "strict", emphasis: "kept" },
  { escape: "strict", emphasis: "alternate" },
  { escape: "strict", emphasis: "none" },
];

/** Attributes that record how something was written rather than what it is. */
const STYLE = new Set(["marker", "tight", "delimiter", "fence", "form", "style"]);

function backtickRun(text: string): number {
  let longest = 0;
  let run = 0;
  for (const character of text) {
    run = character === "`" ? run + 1 : 0;
    longest = Math.max(longest, run);
  }
  return longest;
}

/** What a backslash can escape: ASCII punctuation only. */
const PUNCTUATION = /[!-/:-@[-`{-~]/u;
/** What CommonMark's flanking rules count as punctuation: any Unicode
 *  punctuation or symbol, emoji included. */
const FLANKING_PUNCTUATION = /[\p{P}\p{S}]/u;
const WORD = /[\p{L}\p{N}]/u;
const SPACE = /\s/u;

/** Whether a delimiter character between `before` and `after` could open or
 *  close emphasis, as CommonMark's flanking rules decide. Unknown neighbours
 *  (a text node's edge) count as able to. */
function flanking(before: string | undefined, after: string | undefined, underscore: boolean) {
  if (before === undefined || after === undefined) return true;
  const punctuation = (character: string) => FLANKING_PUNCTUATION.test(character);
  const left =
    !SPACE.test(after) && (!punctuation(after) || SPACE.test(before) || punctuation(before));
  const right =
    !SPACE.test(before) && (!punctuation(before) || SPACE.test(after) || punctuation(after));
  if (underscore && WORD.test(before) && WORD.test(after)) return false;
  return left || right;
}

/** Escapes what Markdown would otherwise read as syntax, and nothing else, so
 *  prose stays readable in any other editor. */
function escapeText(value: string, lineStart: boolean, escaping: Escaping): string {
  if (escaping.escape === "strict") {
    let out = "";
    for (const character of value)
      out += PUNCTUATION.test(character) ? `\\${character}` : character;
    return out;
  }
  const characters = [...value];
  let out = "";
  let atLineStart = lineStart;
  for (let index = 0; index < characters.length; index++) {
    const character = characters[index]!;
    const before = index > 0 ? characters[index - 1] : undefined;
    const after = characters[index + 1];
    let escape = false;
    if (atLineStart) {
      const line = characters.slice(index).join("").split("\n")[0]!;
      escape =
        /^#{1,6}(?:\s|$)/u.test(line) ||
        /^>/u.test(line) ||
        /^[-+*](?:\s|$)/u.test(line) ||
        /^(?:`{3,}|~{3,})/u.test(line) ||
        /^(?:=+|-+)\s*$/u.test(line);
      if (!escape && /^\d{1,9}[.)](?:\s|$)/u.test(line)) {
        const digits = /^\d+/u.exec(line)![0].length;
        out += characters.slice(index, index + digits).join("") + "\\";
        index += digits - 1;
        atLineStart = false;
        continue;
      }
    }
    if (!escape) {
      switch (character) {
        case "\\":
          escape = after === undefined || PUNCTUATION.test(after) || after === "\n";
          break;
        case "`":
          escape = true;
          break;
        case "*":
        case "~":
          escape = flanking(before, after, false);
          break;
        case "_":
          escape = flanking(before, after, true);
          break;
        case "[": {
          const close = characters.indexOf("]", index);
          escape =
            after === "[" ||
            after === "^" ||
            (close !== -1 && ["(", "[", ":"].includes(characters[close + 1] ?? ""));
          break;
        }
        case "]":
          escape = after === "(" || after === "[" || after === ":";
          break;
        case "<":
          escape = after !== undefined && /[A-Za-z/!?]/u.test(after);
          break;
        case "&": {
          ENTITY.lastIndex = 0;
          escape = ENTITY.exec(characters.slice(index, index + 40).join(""))?.index === 0;
          break;
        }
        case "!":
          escape = after === "[";
          break;
      }
    }
    out += escape ? `\\${character}` : character;
    atLineStart = character === "\n";
  }
  return out;
}

function codeSpan(text: string): string {
  const ticks = "`".repeat(backtickRun(text) + 1);
  const pad = /^`|`$/u.test(text) || (/^ .* $/u.test(text) && text.trim() !== "") ? " " : "";
  return `${ticks}${pad}${text}${pad}${ticks}`;
}

function destination(href: string): string {
  return /[\s()<>]/u.test(href) || href === ""
    ? `<${href.replace(/[<>]/gu, (c) => `\\${c}`)}>`
    : href;
}

function linkTitle(title: string | null): string {
  return title ? ` "${title.replace(/["\\]/gu, (c) => `\\${c}`)}"` : "";
}

/** One inline run under a single mark, or a leaf. */
type Span = { span: true; mark: Mark; children: (Span | Node)[] };

/** Groups siblings into nested spans so each mark opens and closes once. */
function spans(nodes: Node[], depth = 0): (Span | Node)[] {
  const result: (Span | Node)[] = [];
  let index = 0;
  while (index < nodes.length) {
    const mark = nodes[index]!.marks[depth];
    if (!mark || mark.type.name === "code") {
      result.push(nodes[index]!);
      index++;
      continue;
    }
    let end = index + 1;
    while (end < nodes.length && nodes[end]!.marks[depth]?.eq(mark)) end++;
    result.push({ span: true, mark, children: spans(nodes.slice(index, end), depth + 1) });
    index = end;
  }
  return result;
}

function plainText(children: (Span | Node)[]): string | null {
  let text = "";
  for (const child of children) {
    if ("span" in child || !child.isText || child.marks.length > 1) return null;
    text += child.text;
  }
  return text;
}

function renderInline(children: (Span | Node)[], escaping: Escaping, lineStart: boolean): string {
  let out = "";
  for (const child of children) {
    const start = lineStart && (out === "" || out.endsWith("\n"));
    if ("span" in child) {
      out += renderSpan(child, escaping, start);
      continue;
    }
    if (child.isText) {
      out += child.marks.some((mark) => mark.type.name === "code")
        ? codeSpan(child.text!)
        : escapeText(child.text!, start, escaping);
      continue;
    }
    switch (child.type.name) {
      case "hardBreak":
        out += child.attrs.style === "spaces" ? "  \n" : "\\\n";
        break;
      case "wikiLink":
        out += `[[${child.attrs.target}${child.attrs.alias ? `|${child.attrs.alias}` : ""}]]`;
        break;
      case "rawInline":
        out += String(child.attrs.source);
        break;
    }
  }
  return out;
}

function renderSpan(span: Span, escaping: Escaping, lineStart: boolean): string {
  const mark = span.mark;
  if (mark.type.name === "link") {
    const text = plainText(span.children);
    const href = String(mark.attrs.href);
    if (text !== null && mark.attrs.form !== "inline" && escaping.escape === "minimal") {
      if (mark.attrs.form === "angle" && !/[\s<>]/u.test(text)) return `<${text}>`;
      if (mark.attrs.form === "bare" && !/\s/u.test(text)) return text;
    }
    return `[${renderInline(span.children, escaping, false)}](${destination(href)}${linkTitle(mark.attrs.title as string | null)})`;
  }
  const inner = renderInline(span.children, escaping, lineStart);
  if (escaping.emphasis === "none") return inner;
  const alternate = escaping.emphasis === "alternate";
  const delimiter =
    mark.type.name === "bold"
      ? alternate
        ? "**"
        : String(mark.attrs.marker ?? "**")
      : mark.type.name === "italic"
        ? alternate
          ? "_"
          : String(mark.attrs.marker ?? "*")
        : mark.type.name === "strike"
          ? alternate
            ? "~~"
            : String(mark.attrs.marker ?? "~~")
          : "";
  // Delimiters cannot sit against whitespace, so it moves outside them.
  const leading = /^\s*/u.exec(inner)![0];
  const trailing = /\s*$/u.exec(inner.slice(leading.length))![0];
  const core = inner.slice(leading.length, inner.length - trailing.length);
  if (!core) return inner;
  return `${leading}${delimiter}${core}${delimiter}${trailing}`;
}

/** Whether `text`, read as a paragraph, holds exactly `node`'s inline content. */
function inlineReadsBack(text: string, node: Node): boolean {
  const reread = parse(text).doc.content ?? [];
  if (reread.length !== 1 || reread[0]!.type !== "paragraph") return false;
  try {
    const content = node.type.schema.nodeFromJSON(reread[0]).toJSON() as JSONContent;
    return sameContent(content, {
      type: "paragraph",
      content: (node.toJSON() as JSONContent).content,
    });
  } catch {
    return false;
  }
}

/** Each emphasis run of a text block: its mark, and the children it covers. */
function emphasisRuns(node: Node): { mark: Mark; from: number; to: number }[] {
  const children: Node[] = [];
  node.forEach((child) => children.push(child));
  const runs: { mark: Mark; from: number; to: number }[] = [];
  for (const name of EMPHASIS) {
    let index = 0;
    while (index < children.length) {
      const mark = children[index]!.marks.find((m) => m.type.name === name);
      if (!mark) {
        index++;
        continue;
      }
      let end = index + 1;
      while (end < children.length && children[end]!.marks.some((m) => m.eq(mark))) end++;
      runs.push({ mark, from: index, to: end });
      index = end;
    }
  }
  return runs;
}

function withoutRun(node: Node, run: { mark: Mark; from: number; to: number }): Node {
  const content: Node[] = [];
  node.forEach((child, _offset, index) =>
    content.push(
      index >= run.from && index < run.to ? child.mark(run.mark.removeFromSet(child.marks)) : child,
    ),
  );
  return node.copy(Fragment.from(content));
}

const MAX_RUNS_TRIED = 16;

/** A paragraph's or heading's text, by the first of `ATTEMPTS` that reads
 *  back as the same content. When none does, emphasis CommonMark cannot
 *  express is given up, then one run at a time, and only then all of it.
 *  Verifying per line of text keeps a failure from costing formatting
 *  anywhere else in a long list. */
function inlineContent(node: Node, strict: boolean): string {
  const write = (candidate: Node, escaping: Escaping) => {
    const children: Node[] = [];
    candidate.forEach((child) => children.push(child));
    // Leading whitespace means nothing in Markdown, but written out it would
    // move a list item's content column or start an indented code block.
    return renderInline(spans(children), escaping, true).replace(/^[ \t]+/u, "");
  };
  const attempt = (candidate: Node): string | null => {
    for (const escaping of ATTEMPTS) {
      if ((strict && escaping.escape === "minimal") || escaping.emphasis === "none") continue;
      const written = write(candidate, escaping);
      if (written === "" || inlineReadsBack(written, candidate)) return written;
    }
    return null;
  };
  const reduced = expressible(node);
  const candidates = reduced === node ? [node] : [node, reduced];
  const runs = emphasisRuns(reduced);
  if (runs.length <= MAX_RUNS_TRIED)
    candidates.push(...runs.map((run) => withoutRun(reduced, run)));
  for (const candidate of candidates) {
    const written = attempt(candidate);
    if (written !== null) return written;
  }
  return write(node, { escape: "strict", emphasis: "none" });
}

function indent(text: string, width: number): string {
  const pad = " ".repeat(width);
  return text
    .split("\n")
    .map((line, index) => (index === 0 || line === "" ? line : pad + line))
    .join("\n");
}

/** Blocks inside a container. Two adjacent lists of one kind would merge
 *  when read back, so the second switches its marker. */
function blocks(parent: Node, strict: boolean, separator = "\n\n"): string {
  const parts: string[] = [];
  let previous: Previous = null;
  parent.forEach((child) => {
    const text = block(child, strict, previous);
    parts.push(text);
    previous = emittedList(child, text);
  });
  return parts.filter((part, index) => part !== "" || index === 0).join(separator);
}

function listTight(node: Node): boolean {
  if (!node.attrs.tight) return false;
  let tight = true;
  node.forEach((item) => {
    let paragraphs = 0;
    item.forEach((child) => {
      if (child.type.name === "paragraph") paragraphs++;
      else if (!child.type.name.endsWith("List")) tight = false;
    });
    if (paragraphs > 1) tight = false;
  });
  return tight;
}

function list(node: Node, strict: boolean, previous: Previous): string {
  const tight = listTight(node);
  const ordered = node.type.name === "orderedList";
  let bullet = String(node.attrs.marker ?? "-");
  let delimiter = String(node.attrs.delimiter ?? ".");
  if (previous?.type === node.type.name) {
    if (!ordered && previous.marker === bullet) bullet = bullet === "-" ? "*" : "-";
    if (ordered && previous.marker === delimiter) delimiter = delimiter === "." ? ")" : ".";
  }
  const start = Number(node.attrs.start ?? 1);
  const items: string[] = [];
  node.forEach((item, _offset, index) => {
    const marker = ordered ? `${start + index}${delimiter} ` : `${bullet} `;
    const task = item.attrs.checked === null ? "" : item.attrs.checked ? "[x] " : "[ ] ";
    const body = blocks(item, strict, tight ? "\n" : "\n\n");
    items.push(marker + task + indent(body, marker.length));
  });
  return items.join(tight ? "\n" : "\n\n");
}

function codeBlock(node: Node): string {
  const text = node.textContent;
  const character = String(node.attrs.fence || "```")[0] === "~" ? "~" : "`";
  const run =
    character === "`"
      ? backtickRun(text)
      : Math.max(0, ...[...text.matchAll(/~+/gu)].map((m) => m[0].length));
  const fence = character.repeat(Math.max(3, run + 1, String(node.attrs.fence || "").length));
  return `${fence}${node.attrs.language ?? ""}\n${text}${text ? "\n" : ""}${fence}`;
}

/** The list written immediately before a block, if any: two lists of one
 *  kind and marker would merge when read back. */
export type Previous = { type: string; marker: string } | null;

/** The list marker `text` was written with, when `node` is a list. */
export function emittedList(node: Node, text: string): Previous {
  if (node.type.name === "bulletList") return { type: node.type.name, marker: text[0] ?? "-" };
  if (node.type.name === "orderedList")
    return { type: node.type.name, marker: /^\d+([.)])/u.exec(text)?.[1] ?? "." };
  return null;
}

function block(node: Node, strict: boolean, previous: Previous = null): string {
  switch (node.type.name) {
    case "paragraph":
      return inlineContent(node, strict);
    case "heading": {
      const text = inlineContent(node, strict).replace(/\n/gu, " ");
      return "#".repeat(Number(node.attrs.level)) + (text ? ` ${text}` : "");
    }
    case "blockquote":
      return blocks(node, strict)
        .split("\n")
        .map((line) => (line ? `> ${line}` : ">"))
        .join("\n");
    case "bulletList":
    case "orderedList":
      return list(node, strict, previous);
    case "codeBlock":
      return codeBlock(node);
    case "horizontalRule":
      return String(node.attrs.marker || "---");
    case "rawBlock":
      return String(node.attrs.source);
    default:
      return "";
  }
}

type Unit = { character: string | null; node: JSONContent | null; marks: JSONContent[] };

/** Inline content in a canonical form. Whitespace carries exactly the marks
 *  of the text on both sides of it: a writer must move it outside a run's
 *  delimiters, and `*a* *b*` means what `*a b*` means. Adjacent text with the
 *  same marks is one run. Code keeps its whitespace. */
function canonicalInline(content: JSONContent[]): JSONContent[] {
  const units: Unit[] = [];
  for (const child of content) {
    const marks = (child.marks ?? []).map((mark) => semantic(mark as JSONContent));
    if (child.type === "text" && child.text !== undefined)
      for (const character of child.text) units.push({ character, node: null, marks });
    else units.push({ character: null, node: semantic(child), marks });
  }
  const code = (unit: Unit) => unit.marks.some((mark) => mark.type === "code");
  const blank = (unit: Unit) =>
    unit.character !== null && /\s/u.test(unit.character) && !code(unit);
  const key = (mark: JSONContent) => JSON.stringify(mark);
  const settled = units.map((unit, index) => {
    if (!blank(unit)) return unit;
    let before: Unit | undefined;
    for (let at = index - 1; at >= 0 && !before; at--) if (!blank(units[at]!)) before = units[at];
    let after: Unit | undefined;
    for (let at = index + 1; at < units.length && !after; at++)
      if (!blank(units[at]!)) after = units[at];
    const shared = (before?.marks ?? []).filter((mark) =>
      (after?.marks ?? []).some((other) => key(other) === key(mark)),
    );
    return { ...unit, marks: shared };
  });
  const result: JSONContent[] = [];
  for (const unit of settled) {
    const last = result.at(-1);
    const marks = unit.marks.length ? { marks: unit.marks as JSONContent["marks"] } : {};
    if (unit.node) result.push({ ...unit.node, ...marks });
    else if (
      last?.type === "text" &&
      JSON.stringify(last.marks ?? []) === JSON.stringify(unit.marks)
    )
      last.text += unit.character!;
    else result.push({ type: "text", text: unit.character!, ...marks });
  }
  return result;
}

/** Content as Markdown can express it: without the attributes that only
 *  record spelling, and without whitespace Markdown does not keep, at the
 *  edges of a line or of a formatted run. */
export function semantic(json: JSONContent): JSONContent {
  const attrs = json.attrs
    ? Object.fromEntries(Object.entries(json.attrs).filter(([key]) => !STYLE.has(key)))
    : undefined;
  const content = json.content;
  if (content && (json.type === "paragraph" || json.type === "heading")) {
    const trimmed = content
      .map((child, index) => {
        if (child.type !== "text" || child.text === undefined) return child;
        let text = child.text.replace(/[ \t]+\n/gu, "\n").replace(/\n[ \t]+/gu, "\n");
        if (index === 0) text = text.trimStart();
        if (index === content!.length - 1) text = text.trimEnd();
        return { ...child, text };
      })
      .filter((child) => child.type !== "text" || child.text !== "");
    return {
      type: json.type,
      ...(attrs && Object.keys(attrs).length ? { attrs } : {}),
      ...(trimmed.length ? { content: canonicalInline(trimmed) } : {}),
    };
  }
  return {
    type: json.type,
    ...(attrs && Object.keys(attrs).length ? { attrs } : {}),
    ...(json.text !== undefined ? { text: json.text } : {}),
    ...(json.marks?.length
      ? { marks: json.marks.map((mark) => semantic(mark as JSONContent)) }
      : {}),
    ...(content?.length ? { content: content.map(semantic) } : {}),
  } as JSONContent;
}

function sameContent(a: JSONContent, b: JSONContent): boolean {
  return JSON.stringify(semantic(a)) === JSON.stringify(semantic(b));
}

/** Whether `markdown` reads back as exactly `node`. */
function readsBackAs(markdown: string, node: Node): boolean {
  const reread = parse(markdown).doc.content ?? [];
  if (reread.length !== 1) return false;
  try {
    // Through the schema, so adjacent text runs join as they do in the editor.
    const normalized = node.type.schema.nodeFromJSON(reread[0]).toJSON() as JSONContent;
    return sameContent(normalized, node.toJSON() as JSONContent);
  } catch {
    return false;
  }
}

const EMPHASIS = new Set(["bold", "italic", "strike"]);

/** The same block without emphasis CommonMark cannot express: a run that
 *  starts with punctuation right after a letter, or ends with punctuation
 *  right before one, as in `**(aside)**note`. Only those runs lose it. */
export function expressible(node: Node): Node {
  if (!node.isTextblock) {
    const children: Node[] = [];
    node.forEach((child) => children.push(expressible(child)));
    return node.copy(Fragment.from(children));
  }
  const children: Node[] = [];
  node.forEach((child) => children.push(child));
  const text = children.map((child) => (child.isText ? child.text! : "\u{fffc}"));
  const drop = children.map(() => new Set<string>());
  for (const name of EMPHASIS) {
    let index = 0;
    while (index < children.length) {
      const mark = children[index]!.marks.find((m) => m.type.name === name);
      if (!mark) {
        index++;
        continue;
      }
      let end = index + 1;
      while (end < children.length && children[end]!.marks.some((m) => m.eq(mark))) end++;
      // By code point: an emoji at the edge is one symbol, not two halves.
      const run = text.slice(index, end).join("");
      const inner = [...run.trim()];
      const strikeEdge = (a: Node | undefined, b: Node | undefined) =>
        name !== "strike" &&
        !!a &&
        !!b &&
        a.marks.some((m) => m.type.name === "strike" && !m.isInSet(b.marks));
      // The reader counts a strikethrough's `~` beside another delimiter as a
      // letter, not punctuation.
      const before = strikeEdge(children[index - 1], children[index])
        ? "a"
        : ([...text.slice(0, index).join("")].at(-1) ?? "");
      const after = strikeEdge(children[end], children[end - 1])
        ? "a"
        : ([...text.slice(end).join("")][0] ?? "");
      const leading = /^\s*/u.exec(run)![0];
      const trailing = /\s*$/u.exec(run)![0];
      const first = inner[0] ?? "";
      const last = inner.at(-1) ?? "";
      const opens = leading !== "" || !FLANKING_PUNCTUATION.test(first) || !WORD.test(before);
      const closes = trailing !== "" || !FLANKING_PUNCTUATION.test(last) || !WORD.test(after);
      if (!opens || !closes) for (let at = index; at < end; at++) drop[at]!.add(name);
      index = end;
    }
  }
  if (drop.every((names) => names.size === 0)) return node;
  const content = children.map((child, index) =>
    drop[index]!.size
      ? child.mark(child.marks.filter((m) => !drop[index]!.has(m.type.name)))
      : child,
  );
  return node.copy(Fragment.from(content));
}

/** A block written from scratch. Its text is verified line by line as it is
 *  written; the block as a whole is verified too, and written with strict
 *  escaping throughout if its structure would not read back. */
export function writeBlock(node: Node, previous: Previous = null): string {
  const written = block(node, false, previous);
  if (written === "" || readsBackAs(written, node) || readsBackAs(written, expressible(node)))
    return written;
  return block(node, true, previous);
}
