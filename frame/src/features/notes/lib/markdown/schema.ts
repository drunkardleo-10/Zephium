import { Mark, Node, mergeAttributes, type Extensions } from "@tiptap/core";
import Document from "@tiptap/extension-document";
import Paragraph from "@tiptap/extension-paragraph";
import Text from "@tiptap/extension-text";
import Heading from "@tiptap/extension-heading";
import Bold from "@tiptap/extension-bold";
import Italic from "@tiptap/extension-italic";
import Strike from "@tiptap/extension-strike";
import Code from "@tiptap/extension-code";
import Blockquote from "@tiptap/extension-blockquote";
import BulletList from "@tiptap/extension-bullet-list";
import OrderedList from "@tiptap/extension-ordered-list";
import ListItem from "@tiptap/extension-list-item";
import CodeBlock from "@tiptap/extension-code-block";
import HorizontalRule from "@tiptap/extension-horizontal-rule";
import HardBreak from "@tiptap/extension-hard-break";

/** How a construct was written, kept so an edited block is written back the
 *  same way. None of these reach the DOM. */
function style<T>(fallback: T) {
  return { default: fallback, rendered: false };
}

const Checkable = ListItem.extend({
  addAttributes() {
    return {
      /** `null` for an ordinary item; a GFM task item otherwise. */
      checked: {
        default: null,
        keepOnSplit: false,
        parseHTML: (element) =>
          element.hasAttribute("data-checked") ? element.dataset.checked === "true" : null,
        renderHTML: (attributes) =>
          attributes.checked === null ? {} : { "data-checked": String(attributes.checked) },
      },
    };
  },
  addKeyboardShortcuts() {
    return {
      // A new item after a task is another task, unchecked.
      Enter: () => {
        const { $from } = this.editor.state.selection;
        for (let depth = $from.depth; depth > 0; depth--) {
          const node = $from.node(depth);
          if (node.type.name === this.name)
            return this.editor.commands.splitListItem(
              this.name,
              node.attrs.checked === null ? {} : { checked: false },
            );
        }
        return false;
      },
      Tab: () => this.editor.commands.sinkListItem(this.name),
      "Shift-Tab": () => this.editor.commands.liftListItem(this.name),
    };
  },
});

const Link = Mark.create({
  name: "link",
  priority: 1000,
  inclusive: false,
  addAttributes() {
    return {
      href: { default: "" },
      title: { default: null },
      /** `inline` for `[text](href)`, `angle` for `<href>`, `bare` for a
       *  GFM literal URL. */
      form: style<"inline" | "angle" | "bare">("inline"),
    };
  },
  parseHTML() {
    return [{ tag: "a[href]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return [
      "a",
      mergeAttributes(HTMLAttributes, { rel: "noopener noreferrer nofollow", draggable: "false" }),
      0,
    ];
  },
});

/** `[[target]]` or `[[target|alias]]`. Resolution belongs to the editor. */
const WikiLink = Node.create({
  name: "wikiLink",
  group: "inline",
  inline: true,
  atom: true,
  selectable: true,
  addAttributes() {
    return { target: { default: "" }, alias: { default: null } };
  },
  parseHTML() {
    return [{ tag: "span[data-wiki-link]" }];
  },
  renderHTML({ node }) {
    return [
      "span",
      { "data-wiki-link": node.attrs.target },
      String(node.attrs.alias ?? node.attrs.target),
    ];
  },
});

/** Inline Markdown this editor does not render, kept byte for byte. */
const RawInline = Node.create({
  name: "rawInline",
  group: "inline",
  inline: true,
  atom: true,
  addAttributes() {
    return { source: { default: "" } };
  },
  parseHTML() {
    return [{ tag: "span[data-raw-inline]" }];
  },
  renderHTML({ node }) {
    return ["span", { "data-raw-inline": "" }, String(node.attrs.source)];
  },
});

/** A block this editor does not render (tables, HTML, footnotes, front
 *  matter), kept byte for byte. */
const RawBlock = Node.create({
  name: "rawBlock",
  group: "block",
  atom: true,
  addAttributes() {
    return { source: { default: "" } };
  },
  parseHTML() {
    return [{ tag: "div[data-raw-block]", preserveWhitespace: "full" }];
  },
  renderHTML({ node }) {
    return ["div", { "data-raw-block": "" }, String(node.attrs.source)];
  },
});

/** The schema of a note. The editor adds behaviour on top; tests and
 *  read-only renderers use it as is. */
export function noteSchemaExtensions(): Extensions {
  return [
    Document,
    Paragraph,
    Text,
    Heading.configure({ levels: [1, 2, 3, 4, 5, 6] }),
    Bold.extend({
      addAttributes: () => ({ marker: style<"**" | "__">("**") }),
    }),
    Italic.extend({
      addAttributes: () => ({ marker: style<"*" | "_">("*") }),
    }),
    Strike.extend({
      addAttributes: () => ({ marker: style<"~~" | "~">("~~") }),
      addKeyboardShortcuts() {
        return { "Mod-Shift-x": () => this.editor.commands.toggleStrike() };
      },
    }),
    Code,
    Link,
    Blockquote,
    BulletList.extend({
      addAttributes: () => ({ marker: style<"-" | "*" | "+">("-"), tight: style(true) }),
    }),
    OrderedList.extend({
      addAttributes() {
        return {
          ...this.parent?.(),
          delimiter: style<"." | ")">("."),
          tight: style(true),
        };
      },
    }),
    Checkable,
    CodeBlock.extend({
      addAttributes() {
        return { ...this.parent?.(), fence: style("```") };
      },
    }),
    HorizontalRule.extend({
      addAttributes: () => ({ marker: style("---") }),
    }),
    HardBreak.extend({
      addAttributes: () => ({ style: style<"backslash" | "spaces">("backslash") }),
    }),
    WikiLink,
    RawInline,
    RawBlock,
  ];
}
