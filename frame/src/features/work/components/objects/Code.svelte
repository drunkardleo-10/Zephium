<script lang="ts">
  import CodeBlock from "$shared/ui/data/Code/CodeBlock.svelte";
  import { codeLines } from "$shared/ui/data/Code";
  import * as m from "$shared/i18n/messages";
  import type { CodeView, ObjectActions } from "../../lib/board/types";
  import { splitPath } from "./path";
  import Title from "./Title.svelte";
  /** An excerpt of a file with its notes: the path set as a header, the lines as the page. */
  let {
    object,
    actions = {},
    centre = false,
  }: {
    object: CodeView;
    actions?: ObjectActions;
    /** Opened in the centre: all of it, at reading size. */
    centre?: boolean;
  } = $props();
  const LINES = 16;
  const count = $derived(codeLines(object.text).length);
  const path = $derived(object.path ? splitPath(object.path) : null);
</script>

<section class="code" aria-label={object.title ?? object.path}>
  <header>
    {#if object.title}<Title text={object.title} />{/if}
    {#if path}<p class="path">
        <span class="folder">{path.folder}</span><span class="name">{path.name}</span>
      </p>{/if}
  </header>
  <div class="page">
    <CodeBlock
      language={object.language}
      text={object.text}
      label={object.title ?? object.path ?? ""}
      notes={object.notes}
      limit={centre ? undefined : LINES}
      variant={centre ? "lift" : "card"}
      start={object.start ?? 1}
    />
  </div>
  {#if count > LINES && !centre}<button
      type="button"
      class="all nodrag nopan"
      onclick={() => actions.open?.(object.id)}>{m.work_object_show_all_lines({ count })}</button
    >{/if}
</section>

<style>
  .code {
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-sizing: border-box;
    inline-size: 100%;
    padding: 16px 18px;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
  }

  header {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .path {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-label);
    overflow-wrap: anywhere;
  }

  .folder {
    color: var(--color-muted);
  }

  .name {
    color: var(--color-text);
    font-weight: 600;
  }

  .page {
    margin-inline: -6px;
  }

  .all {
    align-self: flex-start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
  }

  .all:hover {
    color: var(--color-text);
  }
</style>
