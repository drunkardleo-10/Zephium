import type { JSONContent } from "@tiptap/core";
import type { Token, Tokens } from "marked";
import { decodeEntity, ENTITY } from "./entities";
import { lex } from "./lexer";

/** Where one top-level block came from: its exact text, and the blank lines
 *  that followed it. */
export type SourceBlock = { body: string; gap: string };

export type Parsed = {
  doc: JSONContent;
  /** One entry per top-level node of `doc`, in order. Empty for an empty note. */
  blocks: SourceBlock[];
  /** Blank lines before the first block. */
  lead: string;
};

type MarkJSON = { type: string; attrs?: Record<string, unknown> };

const FRONT_MATTER = /^---\n[\s\S]*?\n(?:---|\.\.\.)[ \t]*(?:\n|$)/u;

function raw(source: string): JSONContent {
  return { type: "rawBlock", attrs: { source } };
}

function rawInline(source: string, marks: MarkJSON[]): JSONContent {
  return { type: "rawInline", attrs: { source }, ...(marks.length ? { marks } : {}) };
}

function withMark(marks: MarkJSON[], mark: MarkJSON): MarkJSON[] {
  // A mark nested in itself cannot be represented; the outer one stands.
  return marks.some((known) => known.type === mark.type) ? marks : [...marks, mark];
}

function text(value: string, marks: MarkJSON[]): JSONContent[] {
  if (!value) return [];
  return [{ type: "text", text: value, ...(marks.length ? { marks: [...marks] } : {}) }];
}

/** Plain text with character references decoded; an unknown reference stays
 *  exactly as written. */
function decoded(value: string, marks: MarkJSON[]): JSONContent[] {
  const nodes: JSONContent[] = [];
  let buffer = "";
  let last = 0;
  ENTITY.lastIndex = 0;
  for (let match = ENTITY.exec(value); match; match = ENTITY.exec(value)) {
    const character = decodeEntity(match);
    buffer += value.slice(last, match.index);
    if (character === null) {
      nodes.push(...text(buffer, marks), rawInline(match[0], marks));
      buffer = "";
    } else buffer += character;
    last = match.index + match[0].length;
  }
  buffer += value.slice(last);
  nodes.push(...text(buffer, marks));
  return nodes;
}

function linkForm(token: Tokens.Link): "inline" | "angle" | "bare" {
  if (token.raw.startsWith("<")) return "angle";
  return token.raw.startsWith("[") ? "inline" : "bare";
}

function inline(tokens: Token[] | undefined, marks: MarkJSON[] = []): JSONContent[] {
  const nodes: JSONContent[] = [];
  for (const token of tokens ?? []) {
    switch (token.type) {
      case "text":
        nodes.push(
          ...("tokens" in token && token.tokens?.length
            ? inline(token.tokens, marks)
            : decoded(token.text, marks)),
        );
        break;
      case "escape":
        nodes.push(...text(token.text, marks));
        break;
      case "strong":
        nodes.push(
          ...inline(
            token.tokens,
            withMark(marks, { type: "bold", attrs: { marker: token.raw.slice(0, 2) } }),
          ),
        );
        break;
      case "em":
        nodes.push(
          ...inline(
            token.tokens,
            withMark(marks, { type: "italic", attrs: { marker: token.raw[0] } }),
          ),
        );
        break;
      case "del":
        nodes.push(
          ...inline(
            token.tokens,
            withMark(marks, {
              type: "strike",
              attrs: { marker: token.raw.startsWith("~~") ? "~~" : "~" },
            }),
          ),
        );
        break;
      case "codespan":
        nodes.push(...text(token.text, withMark(marks, { type: "code" })));
        break;
      case "br":
        nodes.push({
          type: "hardBreak",
          attrs: { style: token.raw.startsWith("\\") ? "backslash" : "spaces" },
          ...(marks.length ? { marks } : {}),
        });
        break;
      case "link": {
        const link = token as Tokens.Link;
        // A reference-style link would need its definition moved with it.
        if (/^\[[^\]]*\](?:\[[^\]]*\])?$/u.test(link.raw)) {
          nodes.push(rawInline(link.raw, marks));
          break;
        }
        nodes.push(
          ...inline(
            link.tokens,
            withMark(marks, {
              type: "link",
              attrs: { href: link.href, title: link.title ?? null, form: linkForm(link) },
            }),
          ),
        );
        break;
      }
      case "checkbox":
        // A loose task item carries its box inside its first paragraph; the
        // item already holds the state.
        break;
      case "wikiLink":
        nodes.push({
          type: "wikiLink",
          attrs: { target: token.target as string, alias: (token.alias as string | null) ?? null },
          ...(marks.length ? { marks } : {}),
        });
        break;
      default:
        // Images, inline HTML, footnote references and anything newer.
        nodes.push(rawInline(token.raw, marks));
    }
  }
  return nodes;
}

function paragraph(tokens: Token[] | undefined): JSONContent {
  const content = inline(tokens);
  return { type: "paragraph", ...(content.length ? { content } : {}) };
}

/** Nested blocks, skipping blank space. `null` if any cannot be represented. */
function children(tokens: Token[]): JSONContent[] | null {
  const nodes: JSONContent[] = [];
  for (const token of tokens) {
    if (token.type === "space") continue;
    const node = block(token, true);
    if (!node) return null;
    nodes.push(node);
  }
  return nodes;
}

function list(token: Tokens.List): JSONContent | null {
  const items: JSONContent[] = [];
  for (const item of token.items) {
    const content: JSONContent[] = [];
    let checked: boolean | null = item.task ? Boolean(item.checked) : null;
    for (const child of item.tokens) {
      if (child.type === "checkbox" || child.type === "space") continue;
      // GFM only recognises a task with text after its box. An empty one, as
      // Obsidian and this editor write it, is still a task.
      const empty = /^\[([ xX])\]\s*$/u.exec(child.raw);
      if (
        content.length === 0 &&
        checked === null &&
        empty &&
        (child.type === "text" || child.type === "paragraph")
      ) {
        checked = empty[1] !== " ";
        content.push({ type: "paragraph" });
        continue;
      }
      if (child.type === "text") {
        content.push(paragraph("tokens" in child && child.tokens ? child.tokens : [child]));
        continue;
      }
      const node = block(child, true);
      if (!node) return null;
      content.push(node);
    }
    if (content.length === 0) content.push({ type: "paragraph" });
    // The editor's list item starts with a paragraph; anything else stays verbatim.
    if (content[0]!.type !== "paragraph") return null;
    items.push({ type: "listItem", attrs: { checked }, content });
  }
  const first = token.items[0]?.raw.trimStart() ?? "";
  if (token.ordered) {
    return {
      type: "orderedList",
      attrs: {
        start: typeof token.start === "number" ? token.start : 1,
        delimiter: /^\d+\)/u.test(first) ? ")" : ".",
        tight: !token.loose,
      },
      content: items,
    };
  }
  return {
    type: "bulletList",
    attrs: { marker: first[0] === "*" || first[0] === "+" ? first[0] : "-", tight: !token.loose },
    content: items,
  };
}

function block(token: Token, nested = false): JSONContent | null {
  switch (token.type) {
    case "heading":
      return {
        type: "heading",
        attrs: { level: token.depth },
        ...(token.tokens?.length ? { content: inline(token.tokens) } : {}),
      };
    case "paragraph":
    case "text":
      return paragraph("tokens" in token && token.tokens ? token.tokens : [token]);
    case "code": {
      const fence =
        token.codeBlockStyle === "indented"
          ? ""
          : (/^ {0,3}(`{3,}|~{3,})/u.exec(token.raw)?.[1] ?? "```");
      return {
        type: "codeBlock",
        attrs: { language: token.lang || null, fence },
        ...(token.text ? { content: [{ type: "text", text: token.text }] } : {}),
      };
    }
    case "blockquote": {
      const content = children(token.tokens ?? []);
      if (!content?.length) return nested ? null : raw(token.raw.replace(/\n+$/u, ""));
      return { type: "blockquote", content };
    }
    case "list": {
      const node = list(token as Tokens.List);
      return node ?? (nested ? null : raw(token.raw.replace(/\n+$/u, "")));
    }
    case "hr":
      return { type: "horizontalRule", attrs: { marker: token.raw.trim() } };
    default:
      // Tables, HTML, link and footnote definitions: kept, not rendered.
      return nested ? null : raw(token.raw.replace(/\n+$/u, ""));
  }
}

export function parse(markdown: string): Parsed {
  const blocks: SourceBlock[] = [];
  const content: JSONContent[] = [];
  let rest = markdown;
  let lead = "";
  const front = FRONT_MATTER.exec(rest);
  if (front) {
    const body = front[0].replace(/\n$/u, "");
    content.push(raw(body));
    blocks.push({ body, gap: front[0].endsWith("\n") ? "\n" : "" });
    rest = rest.slice(front[0].length);
  }
  for (const token of lex(rest)) {
    if (token.type === "space") {
      if (blocks.length) blocks[blocks.length - 1]!.gap += token.raw;
      else lead += token.raw;
      continue;
    }
    const node = block(token)!;
    const body = token.raw.replace(/\n+$/u, "");
    content.push(node);
    blocks.push({ body, gap: token.raw.slice(body.length) });
  }
  return {
    doc: { type: "doc", content: content.length ? content : [{ type: "paragraph" }] },
    blocks,
    lead,
  };
}
