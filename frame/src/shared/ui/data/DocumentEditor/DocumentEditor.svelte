<script lang="ts">
  import { onMount } from "svelte";
  import { Editor, Extension } from "@tiptap/core";
  import Document from "@tiptap/extension-document";
  import Paragraph from "@tiptap/extension-paragraph";
  import Text from "@tiptap/extension-text";
  import { history, undo, redo } from "@tiptap/pm/history";
  import { Plugin } from "@tiptap/pm/state";
  import * as m from "$shared/i18n/messages";
  // Mount/key by document identity. The owner retains drafts and confirms saving;
  // incoming projections must never replace an active editor's unsubmitted text.
  let {
    paragraphs,
    label,
    onedit,
    disabled = false,
  }: {
    paragraphs: readonly string[];
    label: string;
    onedit: (paragraphs: string[]) => void;
    disabled?: boolean;
  } = $props();
  let host: HTMLDivElement;
  let editor = $state.raw<Editor | null>(null);
  let limitReached = $state(false);
  const bounded = (parts: readonly string[]) =>
    parts.length <= 512 &&
    parts.every((p) => p.length <= 16_384) &&
    parts.reduce((n, p) => n + p.length, 0) <= 262_144;
  onMount(() => {
    if (!bounded(paragraphs)) {
      limitReached = true;
      return;
    }
    const instance = new Editor({
      element: host,
      injectCSS: false,
      editable: !disabled,
      extensions: [
        Document,
        Paragraph,
        Text,
        Extension.create({
          name: "boundedHistory",
          addProseMirrorPlugins: () => [
            history({ depth: 100 }),
            new Plugin({
              filterTransaction(transaction) {
                const parts: string[] = [];
                transaction.doc.forEach((node) => parts.push(node.textContent));
                const allowed = bounded(parts);
                limitReached = !allowed;
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
      content: {
        type: "doc",
        content: (paragraphs.length ? paragraphs : [""]).map((text) => ({
          type: "paragraph",
          content: text ? [{ type: "text", text }] : [],
        })),
      },
      editorProps: {
        attributes: {
          role: "textbox",
          "aria-label": label,
          "aria-multiline": "true",
          spellcheck: "true",
        },
      },
      onUpdate: ({ editor }) => {
        const parts: string[] = [];
        editor.state.doc.forEach((node) => parts.push(node.textContent));
        onedit(parts);
      },
    });
    editor = instance;
    return () => {
      instance.destroy();
      editor = null;
    };
  });
  $effect(() => {
    editor?.setEditable(!disabled, false);
  });
</script>

<div class="editor" bind:this={host}></div>
{#if limitReached}<p role="alert">{m.work_document_limit()}</p>{/if}
<p class="hint">{m.work_document_draft()}</p>

<style>
  .editor {
    min-height: 220px;
    max-height: 560px;
    overflow: auto;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: 16px;
    background: var(--color-field);
    color: var(--color-text);
  }

  .editor :global(.tiptap) {
    min-height: 180px;
    outline: none;
    line-height: 1.65;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .editor:focus-within {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .editor :global(p) {
    margin-block: 0 12px;
  }

  .hint,
  p {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }
</style>
