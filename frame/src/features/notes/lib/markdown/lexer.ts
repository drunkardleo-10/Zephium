import { Marked, type Token, type TokenizerExtension } from "marked";

/** `[[target]]` and `[[target|alias]]`, as Obsidian and most Markdown note
 *  tools write links between notes. */
const wikiLink: TokenizerExtension = {
  name: "wikiLink",
  level: "inline",
  start: (source) => source.indexOf("[["),
  tokenizer(source) {
    const match = /^\[\[([^[\]\n|]+?)(?:\|([^[\]\n]+?))?\]\]/u.exec(source);
    if (!match) return undefined;
    return {
      type: "wikiLink",
      raw: match[0],
      target: match[1]!.trim(),
      alias: match[2]?.trim() ?? null,
    };
  },
};

/** Footnote references and definitions are kept verbatim; without these the
 *  lexer would read `[^1]` as a reference link. */
const footnoteReference: TokenizerExtension = {
  name: "footnoteReference",
  level: "inline",
  start: (source) => source.indexOf("[^"),
  tokenizer(source) {
    const match = /^\[\^[^\]\s]+\]/u.exec(source);
    return match ? { type: "footnoteReference", raw: match[0] } : undefined;
  },
};

const footnoteDefinition: TokenizerExtension = {
  name: "footnoteDefinition",
  level: "block",
  start: (source) => source.search(/^\[\^/mu),
  tokenizer(source) {
    const match = /^\[\^[^\]\s]+\]:[^\n]*(?:\n(?:[ \t]+[^\n]*|(?=\n[ \t]+\S)))*\n*/u.exec(source);
    return match ? { type: "footnoteDefinition", raw: match[0] } : undefined;
  },
};

const marked = new Marked({
  gfm: true,
  extensions: [wikiLink, footnoteReference, footnoteDefinition],
});

export function lex(source: string): Token[] {
  return marked.lexer(source);
}
