import type { Node } from "@tiptap/pm/model";
import { Plugin } from "@tiptap/pm/state";
import type { EditorView } from "@tiptap/pm/view";

/** What a token is painted as. */
export const KINDS = ["comment", "string", "number", "keyword"] as const;
type Kind = 0 | 1 | 2 | 3;

type Family = {
  /** Opens a comment that runs to the end of the line. */
  line: "//" | "#" | "--" | null;
  /** `/* … *\/` comments. */
  block: boolean;
  /** `'x'` is a one-character literal (a lifetime otherwise), not a string. */
  char: boolean;
  /** Backquotes quote a string, across lines. */
  backtick: boolean;
};

const FAMILIES: Record<string, Family> = {
  c: { line: "//", block: true, char: false, backtick: true },
  rust: { line: "//", block: true, char: true, backtick: false },
  hash: { line: "#", block: false, char: false, backtick: true },
  dash: { line: "--", block: true, char: false, backtick: false },
  css: { line: null, block: true, char: false, backtick: false },
  json: { line: null, block: false, char: false, backtick: false },
};

const LANGUAGES: Record<string, keyof typeof FAMILIES> = {};
for (const [family, names] of Object.entries({
  c: "js javascript jsx mjs cjs ts typescript tsx java c h cpp c++ cc hpp cs csharp go golang swift kotlin kt kts scala php dart zig groovy jsonc scss less svelte vue",
  rust: "rs rust",
  hash: "py python sh bash zsh shell console fish rb ruby yaml yml toml r perl pl dockerfile make makefile ini conf elixir ex exs nix powershell ps1",
  dash: "sql psql mysql sqlite lua hs haskell elm",
  css: "css",
  json: "json json5",
}))
  for (const name of names.split(" ")) LANGUAGES[name] = family as keyof typeof FAMILIES;

// One set for every family: a word that is a keyword somewhere is rarely a
// plain name anywhere else, and one set keeps the scanner a single lookup.
const KEYWORDS = new Set(
  (
    "abstract and as assert async await break case catch class const continue crate def default defer del delete do dyn elif else enum except export extends extern false final finally fn for from func function go guard if impl implements import in instanceof interface is lambda let loop match mod module mut namespace new nil none not null of or override package pass private protected pub public raise readonly return self select static struct super switch this throw throws trait true try type typeof undefined unless unsafe use using val var void when where while with yield " +
    "None True False Self SELECT FROM WHERE INSERT INTO UPDATE DELETE CREATE TABLE JOIN LEFT RIGHT INNER OUTER ON GROUP BY ORDER LIMIT AND OR NOT NULL AS VALUES SET HAVING DISTINCT"
  ).split(" "),
);

const patterns = new Map<Family, RegExp>();

function pattern(family: Family): RegExp {
  let found = patterns.get(family);
  if (found) return found;
  const comments = [
    family.line === "//" && String.raw`\/\/[^\n]*`,
    family.line === "#" && String.raw`#[^\n]*`,
    family.line === "--" && String.raw`--[^\n]*`,
    family.block && String.raw`\/\*[\s\S]*?(?:\*\/|$)`,
  ].filter(Boolean);
  // A quote that is not closed on its line is not yet a string; painting
  // the rest of the block as one while it is being typed would flicker.
  const strings = [
    String.raw`"(?:[^"\\\n]|\\.)*"`,
    family.char ? String.raw`'(?:[^'\\\n]|\\.)'` : String.raw`'(?:[^'\\\n]|\\.)*'`,
    family.backtick && "`(?:[^`\\\\]|\\\\.)*`",
  ].filter(Boolean);
  found = new RegExp(
    [
      comments.length ? `(${comments.join("|")})` : "(?!)",
      `(${strings.join("|")})`,
      String.raw`(\b(?:0[xX][\da-fA-F_]+|\d[\d_]*(?:\.\d+)?(?:[eE][+-]?\d+)?)\b)`,
      String.raw`([A-Za-z_$][\w$]*)`,
    ].join("|"),
    "g",
  );
  patterns.set(family, found);
  return found;
}

/** The family a fence's info string names, if it names one this knows. */
export function familyOf(language: unknown): Family | null {
  if (typeof language !== "string") return null;
  const name = language.trim().split(/\s/u, 1)[0]!.toLowerCase();
  const family = LANGUAGES[name];
  return family ? FAMILIES[family]! : null;
}

/** Comments, strings, numbers and keywords in `code`, as flat
 *  `[start, end, kind]` triples. Everything else is plain text. */
export function scan(code: string, family: Family): number[] {
  const out: number[] = [];
  const re = pattern(family);
  re.lastIndex = 0;
  for (let match = re.exec(code); match; match = re.exec(code)) {
    const kind: Kind | -1 = match[1]
      ? 0
      : match[2]
        ? 1
        : match[3]
          ? 2
          : KEYWORDS.has(match[4]!)
            ? 3
            : -1;
    if (kind >= 0) out.push(match.index, re.lastIndex, kind);
  }
  return out;
}

type Registry = { highlights: Map<string, Highlight> };

function registry(): Registry | null {
  const css = globalThis.CSS as unknown as Partial<Registry> | undefined;
  return css?.highlights && typeof Highlight === "function" ? (css as Registry) : null;
}

/** One highlight per kind, shared by every editor on the page. */
function shared(css: Registry): Highlight[] {
  return KINDS.map((kind) => {
    const name = `note-code-${kind}`;
    let highlight = css.highlights.get(name);
    if (!highlight) {
      highlight = new Highlight();
      css.highlights.set(name, highlight);
    }
    return highlight;
  });
}

/** Ranges over the text of one code block's element. Tokens come in order
 *  and never overlap, so one walk over its text nodes places them all. */
function rangesFor(element: HTMLElement, tokens: number[]): Range[][] {
  const byKind: Range[][] = KINDS.map(() => []);
  const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
  let node = walker.nextNode() as Text | null;
  let offset = 0;
  const locate = (at: number): [Text, number] | null => {
    while (node && at > offset + node.length) {
      offset += node.length;
      node = walker.nextNode() as Text | null;
    }
    return node ? [node, at - offset] : null;
  };
  for (let index = 0; index < tokens.length; index += 3) {
    const start = locate(tokens[index]!);
    const end = start && locate(tokens[index + 1]!);
    if (!start || !end) break;
    const range = new Range();
    range.setStart(...start);
    range.setEnd(...end);
    byKind[tokens[index + 2]!]!.push(range);
  }
  return byKind;
}

type Painted = { node: Node; element: HTMLElement; ranges: Range[][] };

/** How long typing in a block pauses before its colours catch up. */
const SETTLE = 300;

/** Colours labelled code blocks without touching the document or its DOM:
 *  the ranges are handed to the engine's highlight registry, which paints
 *  them. Runs once a note opens and after edits settle, and then only for
 *  the blocks that changed. Where the registry is missing, code stays
 *  plain. */
export function codeHighlighting(): Plugin {
  return new Plugin({
    view(view) {
      const css = registry();
      if (!css) return {};
      const highlights = shared(css);
      let painted: Painted[] = [];
      let timer: ReturnType<typeof setTimeout> | undefined;

      const remove = (entry: Painted) =>
        entry.ranges.forEach((ranges, kind) => {
          for (const range of ranges) highlights[kind]!.delete(range);
        });

      const paint = (current: EditorView) => {
        timer = undefined;
        if (current.isDestroyed) return;
        const next: Painted[] = [];
        const kept = new Set<Painted>();
        current.state.doc.descendants((node, pos) => {
          if (node.type.name !== "codeBlock") return !node.isTextblock;
          const family = familyOf(node.attrs.language);
          if (!family || !node.textContent) return false;
          const element = (current.nodeDOM(pos) as HTMLElement | null)?.querySelector("code");
          if (!element) return false;
          // An unchanged block keeps its node, and its element keeps the
          // ranges already on it.
          const same = painted.find((entry) => entry.node === node && entry.element === element);
          if (same) {
            kept.add(same);
            next.push(same);
            return false;
          }
          const ranges = rangesFor(element, scan(node.textContent, family));
          ranges.forEach((list, kind) => {
            for (const range of list) highlights[kind]!.add(range);
          });
          next.push({ node, element, ranges });
          return false;
        });
        for (const entry of painted) if (!kept.has(entry)) remove(entry);
        painted = next;
      };

      timer = setTimeout(() => paint(view), 0);
      return {
        update(current, previous) {
          if (current.state.doc === previous.doc) return;
          clearTimeout(timer);
          timer = setTimeout(() => paint(current), SETTLE);
        },
        destroy() {
          clearTimeout(timer);
          for (const entry of painted) remove(entry);
          painted = [];
        },
      };
    },
  });
}
