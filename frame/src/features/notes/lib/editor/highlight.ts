import type { Node } from "@tiptap/pm/model";
import { Plugin } from "@tiptap/pm/state";
import type { EditorView } from "@tiptap/pm/view";
import { CODE_KINDS, languageOf, scanCode } from "$shared/ui/data/Code/tokenize";

/** What a token is painted as: the shared scanner's kinds, one highlight each. */
const KINDS = CODE_KINDS;

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
          const family = languageOf(node.attrs.language);
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
          const ranges = rangesFor(element, scanCode(node.textContent, family));
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
