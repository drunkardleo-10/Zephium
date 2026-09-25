import type { Node } from "@tiptap/pm/model";
import type { NodeView, NodeViewConstructor } from "@tiptap/pm/view";
import { lex } from "../markdown/lexer";

type Labels = { check: string; kept: string };

/** A list item; a task carries a check in front of its text. */
function listItem(labels: Labels): NodeViewConstructor {
  return (initial, view, getPos) => {
    let node = initial;
    const dom = document.createElement("li");
    const box = document.createElement("button");
    box.type = "button";
    box.className = "note-check";
    box.contentEditable = "false";
    box.tabIndex = -1;
    box.setAttribute("role", "checkbox");
    box.setAttribute("aria-label", labels.check);
    const body = document.createElement("div");
    body.className = "note-item";
    dom.append(box, body);
    const sync = () => {
      const task = node.attrs.checked !== null;
      box.hidden = !task;
      if (task) {
        dom.dataset.checked = String(node.attrs.checked);
        box.setAttribute("aria-checked", String(node.attrs.checked));
      } else delete dom.dataset.checked;
    };
    sync();
    // Pressing the box must not move the caret or take focus from the text.
    box.addEventListener("mousedown", (event) => event.preventDefault());
    box.addEventListener("click", () => {
      const position = getPos();
      if (!view.editable || position === undefined) return;
      view.dispatch(
        view.state.tr.setNodeMarkup(position, undefined, {
          ...node.attrs,
          checked: !node.attrs.checked,
        }),
      );
    });
    return {
      dom,
      contentDOM: body,
      update(next: Node) {
        if (next.type !== node.type) return false;
        node = next;
        sync();
        return true;
      },
      ignoreMutation: (mutation) => box.contains(mutation.target as globalThis.Node),
      stopEvent: (event) => box.contains(event.target as globalThis.Node),
    } satisfies NodeView;
  };
}

/** `[[target]]`: its words, and whether a note answers to them. */
function wikiLink(onopen: (target: string) => void): NodeViewConstructor {
  return (initial) => {
    let node = initial;
    const dom = document.createElement("span");
    dom.className = "note-wiki";
    dom.contentEditable = "false";
    const render = () => {
      dom.dataset.wikiLink = String(node.attrs.target);
      dom.textContent = String(node.attrs.alias ?? node.attrs.target);
    };
    render();
    dom.addEventListener("mousedown", (event) => {
      if (event.button === 0) event.preventDefault();
    });
    dom.addEventListener("click", () => onopen(String(node.attrs.target)));
    return {
      dom,
      update(next: Node) {
        if (next.type !== node.type) return false;
        const changed = next.attrs.target !== node.attrs.target;
        node = next;
        render();
        if (changed) delete dom.dataset.state;
        return true;
      },
      ignoreMutation: () => true,
    } satisfies NodeView;
  };
}

/** Markdown this editor does not render, shown as written. A table is drawn
 *  as a table; the file keeps its source either way. */
function rawBlock(labels: Labels): NodeViewConstructor {
  return (node) => {
    const dom = document.createElement("div");
    dom.className = "note-raw";
    dom.contentEditable = "false";
    dom.title = labels.kept;
    const source = String(node.attrs.source);
    const table = lex(source).find((token) => token.type === "table");
    if (table && "header" in table) {
      const element = document.createElement("table");
      const head = element.createTHead().insertRow();
      for (const cell of table.header as { text: string }[]) {
        const th = document.createElement("th");
        th.textContent = cell.text;
        head.append(th);
      }
      const body = element.createTBody();
      for (const row of table.rows as { text: string }[][]) {
        const tr = body.insertRow();
        for (const cell of row) tr.insertCell().textContent = cell.text;
      }
      dom.classList.add("is-table");
      dom.append(element);
    } else {
      const pre = document.createElement("pre");
      pre.textContent = source;
      dom.append(pre);
    }
    return {
      dom,
      ignoreMutation: () => true,
      update: (next) => next.attrs.source === source,
    } satisfies NodeView;
  };
}

function rawInline(labels: Labels): NodeViewConstructor {
  return (node) => {
    const dom = document.createElement("span");
    dom.className = "note-raw-inline";
    dom.contentEditable = "false";
    dom.title = labels.kept;
    dom.textContent = String(node.attrs.source);
    return { dom, ignoreMutation: () => true } satisfies NodeView;
  };
}

export function nodeViews(
  labels: Labels,
  onopen: (target: string) => void,
): Record<string, NodeViewConstructor> {
  return {
    listItem: listItem(labels),
    wikiLink: wikiLink(onopen),
    rawBlock: rawBlock(labels),
    rawInline: rawInline(labels),
  };
}

/** Every `[[target]]` in a document, once each. */
export function wikiTargets(doc: Node): string[] {
  const targets: string[] = [];
  doc.descendants((node) => {
    if (node.type.name === "wikiLink") {
      const target = String(node.attrs.target);
      if (!targets.includes(target)) targets.push(target);
      return false;
    }
    return !node.isTextblock || node.childCount > 0;
  });
  return targets;
}
