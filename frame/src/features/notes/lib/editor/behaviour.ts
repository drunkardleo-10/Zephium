import { Extension, InputRule, type Editor } from "@tiptap/core";
import { history, redo, undo } from "@tiptap/pm/history";
import { Fragment, Slice, type Node, type Schema } from "@tiptap/pm/model";
import { Plugin, PluginKey, TextSelection, type EditorState } from "@tiptap/pm/state";
import { Decoration, DecorationSet, type EditorView } from "@tiptap/pm/view";
import { MarkdownDocument } from "../markdown/document";
import { codeHighlighting } from "./highlight";

/** Text typed after `[[` or at the start of a line after `/`, while the
 *  editor offers what it could become. */
export type Trigger = { kind: "link" | "command"; from: number; to: number; query: string };

type TriggerState = { trigger: Trigger | null; dismissed: number | null };

export const triggerKey = new PluginKey<TriggerState>("noteTrigger");

function findTrigger(state: EditorState): Trigger | null {
  const { selection } = state;
  if (!selection.empty || !(selection instanceof TextSelection)) return null;
  const $from = selection.$from;
  if (!$from.parent.isTextblock || $from.parent.type.spec.code) return null;
  const before = $from.parent.textBetween(0, $from.parentOffset, undefined, "￼");
  const link = /\[\[([^[\]\n|￼]{0,80})$/u.exec(before);
  if (link)
    return {
      kind: "link",
      from: $from.pos - link[0].length,
      to: $from.pos,
      query: link[1]!,
    };
  const command = /^\/([\p{L}\p{N} ]{0,24})$/u.exec(before);
  if (command)
    return {
      kind: "command",
      from: $from.pos - command[0].length,
      to: $from.pos,
      query: command[1]!,
    };
  return null;
}

/** Keeps the current trigger in plugin state. Escape dismisses it until the
 *  text before the cursor changes. */
function triggers(keydown: { current: ((event: KeyboardEvent) => boolean) | null }) {
  return new Plugin<TriggerState>({
    key: triggerKey,
    state: {
      init: () => ({ trigger: null, dismissed: null }),
      apply(tr, previous, _old, state) {
        const dismissed =
          tr.getMeta(triggerKey) === "dismiss" ? state.selection.from : previous.dismissed;
        const trigger = findTrigger(state);
        if (trigger && dismissed === trigger.to) return { trigger: null, dismissed };
        return { trigger, dismissed: trigger ? null : dismissed };
      },
    },
    props: {
      handleKeyDown(view, event) {
        if (!triggerKey.getState(view.state)?.trigger) return false;
        if (event.key === "Escape") {
          view.dispatch(view.state.tr.setMeta(triggerKey, "dismiss"));
          return true;
        }
        return keydown.current?.(event) ?? false;
      },
    },
  });
}

/** "Title" on an empty first heading, and what to do next under a title
 *  with nothing below it: on the empty line there, or, before that line
 *  exists, as a hint that makes it when pressed. */
function placeholders(title: string, body: string) {
  const hint = (view: EditorView) => {
    const element = document.createElement("p");
    element.className = "note-hint";
    element.contentEditable = "false";
    element.textContent = body;
    element.addEventListener("mousedown", (event) => {
      event.preventDefault();
      const { state } = view;
      const end = state.doc.content.size;
      const tr = state.tr.insert(end, state.schema.nodes.paragraph!.create());
      view.dispatch(tr.setSelection(TextSelection.create(tr.doc, end + 1)));
      view.focus();
    });
    return element;
  };
  return new Plugin({
    props: {
      decorations(state) {
        const { doc } = state;
        const decorations: Decoration[] = [];
        const first = doc.firstChild;
        if (first?.type.name !== "heading") return null;
        if (first.content.size === 0)
          decorations.push(
            Decoration.node(0, first.nodeSize, { class: "is-empty", "data-placeholder": title }),
          );
        if (doc.childCount === 1)
          decorations.push(
            Decoration.widget(doc.content.size, hint, {
              side: 1,
              key: "hint",
              ignoreSelection: true,
            }),
          );
        const second = doc.childCount === 2 ? doc.child(1) : null;
        if (second?.type.name === "paragraph" && second.content.size === 0) {
          const from = first.nodeSize;
          decorations.push(
            Decoration.node(from, from + second.nodeSize, {
              class: "is-empty",
              "data-placeholder": body,
            }),
          );
        }
        return decorations.length ? DecorationSet.create(doc, decorations) : null;
      },
    },
  });
}

/** Whether pasted plain text is written in Markdown rather than prose that
 *  happens to contain a symbol. */
function looksLikeMarkdown(text: string): boolean {
  return (
    /^\s{0,3}(?:#{1,6}\s|[-+*]\s|\d{1,9}[.)]\s|>\s?|```|~~~|- \[[ xX]\])/mu.test(text) ||
    /\*\*[^*\n]+\*\*|\[[^\]\n]+\]\([^)\s]+\)|\[\[[^\]\n]+\]\]|`[^`\n]+`/u.test(text)
  );
}

function markdownSlice(schema: Schema, text: string): Slice | null {
  try {
    const doc = schema.nodeFromJSON(new MarkdownDocument(text).json);
    // A single paragraph pastes into the line it lands on rather than as a block.
    if (doc.childCount === 1 && doc.firstChild!.type.name === "paragraph")
      return new Slice(doc.firstChild!.content, 0, 0);
    return new Slice(doc.content, 1, 1);
  } catch {
    return null;
  }
}

/** A copied selection as Markdown, for anything that takes plain text. */
function markdownOf(schema: Schema, slice: Slice): string {
  let content = slice.content;
  if (content.firstChild?.isInline)
    content = Fragment.from(schema.nodes.paragraph!.create(null, content));
  try {
    const doc = schema.topNodeType.create(null, content);
    return new MarkdownDocument("").serialize(doc).replace(/\n$/u, "");
  } catch {
    return slice.content.textBetween(0, slice.content.size, "\n\n");
  }
}

function inListItem(state: EditorState): { pos: number; node: Node } | null {
  const { $from } = state.selection;
  for (let depth = $from.depth; depth > 0; depth--) {
    const node = $from.node(depth);
    if (node.type.name === "listItem") return { pos: $from.before(depth), node };
  }
  return null;
}

/** Turns the current line or selection into a checklist, or back to text. */
export function toggleChecklist(editor: Editor): boolean {
  const item = inListItem(editor.state);
  if (item && item.node.attrs.checked !== null)
    return editor.chain().focus().liftListItem("listItem").run();
  if (!item && !editor.chain().focus().toggleBulletList().run()) return false;
  const { state } = editor;
  const { from, to } = state.selection;
  const tr = state.tr;
  state.doc.nodesBetween(from, to, (node, pos) => {
    if (node.type.name === "listItem" && node.attrs.checked === null)
      tr.setNodeMarkup(pos, undefined, { ...node.attrs, checked: false });
  });
  const current = inListItem(state);
  if (current && current.node.attrs.checked === null)
    tr.setNodeMarkup(current.pos, undefined, { ...current.node.attrs, checked: false });
  editor.view.dispatch(tr);
  return true;
}

function toggleChecked(editor: Editor): boolean {
  const item = inListItem(editor.state);
  if (!item || item.node.attrs.checked === null) return false;
  editor.view.dispatch(
    editor.state.tr.setNodeMarkup(item.pos, undefined, {
      ...item.node.attrs,
      checked: !item.node.attrs.checked,
    }),
  );
  return true;
}

export type BehaviourOptions = {
  title: string;
  body: string;
  keydown: { current: ((event: KeyboardEvent) => boolean) | null };
  onlink: () => void;
};

/** Everything the editor does beyond the schema. */
export function behaviour(options: BehaviourOptions) {
  return Extension.create({
    name: "noteBehaviour",
    addProseMirrorPlugins() {
      const schema = this.editor.schema;
      return [
        history({ depth: 200, newGroupDelay: 600 }),
        triggers(options.keydown),
        placeholders(options.title, options.body),
        codeHighlighting(),
        new Plugin({
          props: {
            clipboardTextSerializer: (slice) => markdownOf(schema, slice),
            handlePaste(view, event) {
              const text = event.clipboardData?.getData("text/plain") ?? "";
              const html = event.clipboardData?.getData("text/html") ?? "";
              if (!text || html || !looksLikeMarkdown(text)) return false;
              if (view.state.selection.$from.parent.type.spec.code) return false;
              const slice = markdownSlice(schema, text);
              if (!slice) return false;
              view.dispatch(view.state.tr.replaceSelection(slice).scrollIntoView());
              return true;
            },
          },
        }),
      ];
    },
    addKeyboardShortcuts() {
      return {
        "Mod-z": () => undo(this.editor.state, this.editor.view.dispatch),
        "Mod-Shift-z": () => redo(this.editor.state, this.editor.view.dispatch),
        "Mod-y": () => redo(this.editor.state, this.editor.view.dispatch),
        "Mod-Shift-l": () => toggleChecklist(this.editor),
        "Mod-Shift-u": () => toggleChecked(this.editor),
        "Mod-k": () => {
          options.onlink();
          return true;
        },
        "Mod-Alt-0": () => this.editor.chain().focus().setParagraph().run(),
      };
    },
    addInputRules() {
      const { schema } = this.editor;
      return [
        // `[ ] ` or `[x] ` at the start of a list item makes it a task; at the
        // start of a line, a checklist.
        new InputRule({
          find: /^\s*\[([ xX]?)\]\s$/u,
          handler: ({ state, range, match }) => {
            const checked = (match[1] ?? "").toLowerCase() === "x";
            const item = inListItem(state);
            const tr = state.tr.delete(range.from, range.to);
            if (item && state.selection.$from.parent === item.node.firstChild) {
              tr.setNodeMarkup(item.pos, undefined, { ...item.node.attrs, checked });
              return;
            }
            const $start = tr.doc.resolve(range.from);
            const blockRange = $start.blockRange();
            if (!blockRange) return null;
            tr.wrap(blockRange, [
              { type: schema.nodes.bulletList! },
              { type: schema.nodes.listItem!, attrs: { checked } },
            ]);
          },
        }),
        new InputRule({
          find: /\[\[([^[\]\n|]+)(?:\|([^[\]\n]+))?\]\]$/u,
          handler: ({ state, range, match }) => {
            state.tr.replaceWith(
              range.from,
              range.to,
              schema.nodes.wikiLink!.create({
                target: match[1]!.trim(),
                alias: match[2]?.trim() ?? null,
              }),
            );
          },
        }),
        // A web address typed into prose becomes a link when it ends.
        new InputRule({
          find: /(?:^|\s)((?:https?:\/\/|www\.)[^\s<]*[^\s<.,:;"')\]])\s$/u,
          handler: ({ state, range, match }) => {
            // The space that ended the address is not in the document yet;
            // a rule that changes anything must type it itself.
            const url = match[1]!;
            const end = range.to;
            const start = end - url.length;
            if (state.doc.rangeHasMark(start, end, schema.marks.link!)) return null;
            state.tr
              .addMark(
                start,
                end,
                schema.marks.link!.create({
                  href: url.startsWith("www.") ? `http://${url}` : url,
                  form: "bare",
                }),
              )
              .insertText(" ", end);
          },
        }),
      ];
    },
  });
}
