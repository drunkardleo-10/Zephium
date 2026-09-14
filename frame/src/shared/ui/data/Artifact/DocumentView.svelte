<script lang="ts">
  import type {
    DocumentNodeView as DocumentNode,
    NoteDocumentView as NoteDocument,
  } from "./artifact";
  import DocumentView from "./DocumentView.svelte";
  let {
    document,
    node,
    onlink,
  }: {
    document?: NoteDocument;
    node?: DocumentNode;
    /** Links open through a native intent; chrome never navigates itself. */
    onlink?: (href: string) => void;
  } = $props();
  const root = $derived(node ?? document?.document);
  const children = $derived(root?.content ?? []);
  function href(text: DocumentNode): string | null {
    const link = text.marks?.find((mark) => mark.type === "link");
    return link?.attrs?.href ?? null;
  }
  function styled(text: DocumentNode, kind: string): boolean {
    return text.marks?.some((mark) => mark.type === kind) ?? false;
  }
</script>

{#snippet inline(nodes: readonly DocumentNode[])}
  {#each nodes as text, index (index)}
    {#if text.type === "hardBreak"}<br />
    {:else if text.type === "text"}
      {@const link = href(text)}
      {#if link}<button
          type="button"
          class="link"
          class:bold={styled(text, "bold")}
          class:italic={styled(text, "italic")}
          title={link}
          onclick={() => onlink?.(link)}>{text.text}</button
        >
      {:else if styled(text, "code")}<code>{text.text}</code>
      {:else}<span class:bold={styled(text, "bold")} class:italic={styled(text, "italic")}
          >{text.text}</span
        >{/if}
    {/if}
  {/each}
{/snippet}

{#if root}
  {#each children as child, index (index)}
    {#if child.type === "paragraph"}<p>{@render inline(child.content ?? [])}</p>
    {:else if child.type === "heading"}
      {#if child.attrs?.level === 1}<h3>{@render inline(child.content ?? [])}</h3>
      {:else if child.attrs?.level === 2}<h4>{@render inline(child.content ?? [])}</h4>
      {:else}<h5>{@render inline(child.content ?? [])}</h5>{/if}
    {:else if child.type === "blockquote"}<blockquote>
        <DocumentView node={child} {onlink} />
      </blockquote>
    {:else if child.type === "bulletList" || child.type === "orderedList"}
      <svelte:element this={child.type === "orderedList" ? "ol" : "ul"}>
        {#each child.content ?? [] as item, itemIndex (itemIndex)}
          <li><DocumentView node={item} {onlink} /></li>
        {/each}
      </svelte:element>
    {:else if child.type === "codeBlock"}<pre><code
          >{(child.content ?? []).map((text) => text.text ?? "").join("")}</code
        ></pre>
    {/if}
  {/each}
{/if}

<style>
  p,
  blockquote,
  ul,
  ol,
  pre {
    margin: 0 0 10px;
  }

  h3,
  h4,
  h5 {
    margin: 12px 0 6px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  h3 {
    font-size: var(--text-title);
  }

  h4 {
    font-size: var(--text-body);
  }

  h5 {
    font-size: var(--text-caption);
    color: var(--color-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  blockquote {
    padding-inline-start: 10px;
    border-inline-start: 2px solid var(--color-border-strong);
    color: var(--color-muted);
  }

  ul,
  ol {
    padding-inline-start: 20px;
  }

  li :global(p:last-child) {
    margin-block-end: 2px;
  }

  pre {
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--color-fill);
    overflow: auto;
    font-size: var(--text-caption);
  }

  code {
    padding: 0 3px;
    border-radius: 4px;
    background: var(--color-fill);
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 0.92em;
  }

  .bold {
    font-weight: 600;
  }

  .italic {
    font-style: italic;
  }

  .link {
    display: inline;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-info);
    font: inherit;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--color-info) 40%, transparent);
    text-underline-offset: 2px;
    cursor: default;
  }

  .link:hover {
    text-decoration-color: currentcolor;
  }

  .link:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
    border-radius: 3px;
  }
</style>
