<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { Editor, Extension, Node, type JSONContent } from "@tiptap/core";
  import Document from "@tiptap/extension-document";
  import Paragraph from "@tiptap/extension-paragraph";
  import Text from "@tiptap/extension-text";
  import Heading from "@tiptap/extension-heading";
  import Bold from "@tiptap/extension-bold";
  import Italic from "@tiptap/extension-italic";
  import Code from "@tiptap/extension-code";
  import BulletList from "@tiptap/extension-bullet-list";
  import OrderedList from "@tiptap/extension-ordered-list";
  import ListItem from "@tiptap/extension-list-item";
  import Blockquote from "@tiptap/extension-blockquote";
  import CodeBlock from "@tiptap/extension-code-block";
  import HardBreak from "@tiptap/extension-hard-break";
  import { history, undo, redo } from "@tiptap/pm/history";
  import { Plugin } from "@tiptap/pm/state";
  import { noteReferences } from "$domain/resources";
  import type { NoteDocument, ResourceSummary } from "$domain/resources";
  import { documentProjector } from "../lib/project-document";
  import * as m from "$shared/i18n/messages";
  let {
    value,
    disabled = false,
    onchange,
    onopen,
    findNotes,
    resolveNotes,
    referencesRevision = 0,
  }: {
    value: NoteDocument;
    disabled?: boolean;
    onchange: (value: NoteDocument) => void;
    onopen: (id: string) => void;
    findNotes: (query: string) => Promise<ResourceSummary[]>;
    resolveNotes: (ids: string[]) => Promise<ResourceSummary[]>;
    referencesRevision?: number;
  } = $props();
  let ids = $state<readonly string[]>(untrack(() => noteReferences(value)));
  let referenceKey = $derived(ids.join(","));
  let resolutionKey = $derived(`${referencesRevision}/${referenceKey}`);
  let labels = $state.raw<Record<string, string>>({});
  $effect(() => {
    void resolutionKey;
    const keys = untrack(() => referenceKey);
    let live = true;
    if (!keys) {
      labels = {};
      return;
    }
    void untrack(() => resolveNotes(keys.split(","))).then(
      (rows) => {
        if (live) labels = Object.fromEntries(rows.map((row) => [row.id, row.title]));
      },
      () => {
        if (live) labels = {};
      },
    );
    return () => {
      live = false;
    };
  });
  $effect(() => {
    const names = labels;
    if (editor && host)
      for (const reference of host.querySelectorAll<HTMLElement>("[data-note-reference]"))
        reference.textContent =
          names[reference.dataset.noteReference ?? ""] ?? m.note_unavailable_link();
  });
  let host: HTMLDivElement;
  let editor = $state.raw<Editor | null>(null);
  let transaction = $state(0);
  let limit = $state(false);
  let linking = $state(false);
  let query = $state("");
  let matches = $state.raw<ResourceSummary[]>([]);
  let finding = $state(false);
  let failed = $state(false);
  let format = $derived.by(() => {
    void transaction;
    return {
      bold: editor?.isActive("bold") ?? false,
      italic: editor?.isActive("italic") ?? false,
      heading: editor?.isActive("heading", { level: 2 }) ?? false,
      bullets: editor?.isActive("bulletList") ?? false,
      numbered: editor?.isActive("orderedList") ?? false,
      quote: editor?.isActive("blockquote") ?? false,
      code: editor?.isActive("codeBlock") ?? false,
    };
  });
  $effect(() => {
    if (!linking) return;
    const term = query;
    let live = true;
    finding = true;
    failed = false;
    const timer = setTimeout(() => {
      void findNotes(term).then(
        (rows) => {
          if (live) {
            matches = rows;
            finding = false;
          }
        },
        () => {
          if (live) {
            failed = true;
            finding = false;
          }
        },
      );
    }, 180);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  });
  onMount(() => {
    const project = documentProjector();
    const reference = Node.create({
      name: "noteReference",
      group: "inline",
      inline: true,
      atom: true,
      selectable: true,
      addAttributes: () => ({ resource: { default: null } }),
      parseHTML: () => [],
      addNodeView:
        () =>
        ({ node }) => {
          const dom = document.createElement("span");
          const id = String(node.attrs.resource ?? "");
          dom.dataset.noteReference = id;
          dom.setAttribute("role", "button");
          dom.tabIndex = 0;
          dom.contentEditable = "false";
          dom.textContent = untrack(() => labels[id]) ?? m.note_unavailable_link();
          return { dom, ignoreMutation: (mutation) => mutation.type !== "selection" };
        },
      renderHTML: ({ node }) => [
        "span",
        { "data-note-reference": node.attrs.resource, role: "button", tabindex: "0" },
        m.note_linked_note(),
      ],
    });
    const instance = new Editor({
      element: host,
      injectCSS: false,
      editable: !disabled,
      extensions: [
        Document,
        Paragraph,
        Text,
        Heading.configure({ levels: [1, 2, 3] }),
        Bold,
        Italic,
        Code,
        BulletList,
        OrderedList,
        ListItem,
        Blockquote,
        CodeBlock,
        HardBreak,
        reference,
        Extension.create({
          name: "noteHistory",
          addProseMirrorPlugins: () => [
            history({ depth: 100 }),
            new Plugin({
              filterTransaction(change) {
                if (!change.docChanged) return true;
                const { allowed } = project(change.doc);
                limit = !allowed;
                return allowed;
              },
            }),
          ],
          addKeyboardShortcuts() {
            return {
              "Mod-z": () => undo(this.editor.state, this.editor.view.dispatch),
              "Mod-Shift-z": () => redo(this.editor.state, this.editor.view.dispatch),
            };
          },
        }),
      ],
      content: value.document as JSONContent,
      editorProps: {
        attributes: {
          role: "textbox",
          "aria-label": m.tool_notes(),
          "aria-multiline": "true",
          spellcheck: "true",
        },
        handleClick: (_view, _pos, event) => {
          const target = (event.target as HTMLElement).closest<HTMLElement>(
            "[data-note-reference]",
          );
          const id = target?.dataset.noteReference;
          if (id) {
            onopen(id);
            return true;
          }
          return false;
        },
        handleKeyDown: (_view, event) => {
          const target = event.target as HTMLElement;
          if (event.key === "Enter" && target.dataset.noteReference) {
            onopen(target.dataset.noteReference);
            return true;
          }
          return false;
        },
      },
      onUpdate: ({ editor }) => {
        const next = project(editor.state.doc);
        ids = next.references;
        onchange(next.document);
      },
      onTransaction: ({ transaction: change }) => {
        if (change.docChanged || change.selectionSet || change.storedMarksSet) transaction++;
      },
    });
    editor = instance;
    return () => {
      instance.destroy();
      editor = null;
    };
  });
  $effect(() => editor?.setEditable(!disabled, false));
</script>

<div class="note-toolbar" role="group" aria-label={m.note_formatting()}>
  <button
    class="format-control"
    type="button"
    {disabled}
    aria-label={m.note_bold()}
    title={m.note_bold()}
    aria-pressed={format.bold}
    onclick={() => editor?.chain().focus().toggleBold().run()}
    ><span aria-hidden="true">B</span></button
  >
  <button
    class="format-control"
    type="button"
    {disabled}
    aria-label={m.note_italic()}
    title={m.note_italic()}
    aria-pressed={format.italic}
    onclick={() => editor?.chain().focus().toggleItalic().run()}
    ><span aria-hidden="true">I</span></button
  >
  <button
    class="format-control"
    type="button"
    {disabled}
    aria-label={m.note_heading()}
    title={m.note_heading()}
    aria-pressed={format.heading}
    onclick={() => editor?.chain().focus().toggleHeading({ level: 2 }).run()}
    ><span aria-hidden="true">H₂</span></button
  >
  <button
    class="format-control"
    type="button"
    {disabled}
    aria-label={m.note_bullets()}
    title={m.note_bullets()}
    aria-pressed={format.bullets}
    onclick={() => editor?.chain().focus().toggleBulletList().run()}
    ><span aria-hidden="true">• ≡</span></button
  >
  <button
    class="format-control"
    type="button"
    {disabled}
    aria-label={m.note_numbered()}
    title={m.note_numbered()}
    aria-pressed={format.numbered}
    onclick={() => editor?.chain().focus().toggleOrderedList().run()}
    ><span aria-hidden="true">1.</span></button
  >
  <button
    class="format-control"
    type="button"
    {disabled}
    aria-label={m.note_quote()}
    title={m.note_quote()}
    aria-pressed={format.quote}
    onclick={() => editor?.chain().focus().toggleBlockquote().run()}
    ><span aria-hidden="true">“</span></button
  >
  <button
    class="format-control"
    type="button"
    {disabled}
    aria-label={m.note_code()}
    title={m.note_code()}
    aria-pressed={format.code}
    onclick={() => editor?.chain().focus().toggleCodeBlock().run()}
    ><span aria-hidden="true">&lt;/&gt;</span></button
  >
  <button
    class="format-control"
    type="button"
    {disabled}
    aria-label={m.note_link()}
    title={m.note_link()}
    aria-expanded={linking}
    onclick={() => (linking = !linking)}><span aria-hidden="true">↗</span></button
  >
</div>
{#if linking}<section class="note-links" aria-label={m.note_link()}>
    <label>{m.note_find()}<input type="search" bind:value={query} maxlength="512" /></label
    >{#if finding}<p role="status">{m.surface_loading()}</p>{/if}{#if failed}<p role="alert">
        {m.resource_load_failed()}
      </p>{/if}
    <ul>
      {#each matches as note (note.id)}<li>
          <button
            type="button"
            onclick={() => {
              editor
                ?.chain()
                .focus()
                .insertContent({ type: "noteReference", attrs: { resource: note.id } })
                .run();
              linking = false;
            }}>{note.title}</button
          >
        </li>{/each}
    </ul>
  </section>{/if}
<div class="note-document" bind:this={host}></div>
{#if limit}<p role="alert">{m.work_document_limit()}</p>{/if}

<style>
  .note-toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    padding-block: 6px;
    border-block: 1px solid var(--color-border);
  }

  .note-document {
    min-height: 260px;
    padding-block: 20px;
    color: var(--color-text);
  }

  .note-document :global(.tiptap) {
    min-height: 240px;
    outline: none;
    line-height: 1.7;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .note-document:focus-within {
    box-shadow: inset 2px 0 var(--color-ring);
    outline-offset: 4px;
  }

  .note-document :global(p) {
    margin-block: 0 12px;
  }

  .note-document :global(h1),
  .note-document :global(h2),
  .note-document :global(h3) {
    line-height: 1.3;
    font-weight: 600;
    margin-block: 24px 12px;
  }

  .note-document :global(h1) {
    font-size: var(--text-title);
  }

  .note-document :global(h2) {
    font-size: 20px;
  }

  .note-document :global(h3) {
    font-size: 17px;
  }

  .note-document :global(ul) {
    list-style: disc;
  }

  .note-document :global(ol) {
    list-style: decimal;
  }

  .note-document :global(ul),
  .note-document :global(ol) {
    padding-inline-start: 24px;
  }

  .note-document :global(blockquote) {
    border-inline-start: 2px solid var(--color-border-strong);
    padding-inline-start: 16px;
    color: var(--color-muted);
  }

  .note-document :global(pre) {
    padding: 16px;
    background: var(--color-fill);
    border-radius: var(--radius-control);
    overflow: auto;
  }

  .note-document :global([data-note-reference]) {
    display: inline;
    padding: 2px 6px;
    border-radius: var(--radius-row);
    background: var(--color-accent-soft);
    color: var(--color-accent);
    cursor: pointer;
  }

  .note-links {
    padding: 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
  }

  .note-links label {
    display: grid;
    gap: 8px;
    font-size: var(--text-caption);
    color: var(--color-muted);
  }

  .note-links input {
    background: var(--color-field);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    padding: 8px;
    border-radius: var(--radius-row);
    font: inherit;
  }

  .note-links ul {
    max-height: 180px;
    overflow: auto;
    list-style: none;
    padding: 0;
  }

  .note-links button {
    width: 100%;
    text-align: start;
    padding: 8px;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    border: 0;
    cursor: pointer;
  }

  .note-links button:hover {
    background: var(--color-fill);
  }

  .note-links button:focus-visible,
  .note-links input:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .format-control {
    display: grid;
    place-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-row);
    font: inherit;
    font-weight: 600;
    color: var(--color-muted);
    background: transparent;
    cursor: default;
  }

  .format-control:hover,
  .format-control[aria-pressed="true"],
  .format-control[aria-expanded="true"] {
    color: var(--color-text);
    background: var(--color-fill-active);
  }

  .format-control:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .format-control:disabled {
    opacity: 0.5;
  }
</style>
