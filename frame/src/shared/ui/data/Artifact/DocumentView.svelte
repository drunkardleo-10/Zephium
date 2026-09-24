<script lang="ts">
  import {
    documentDigest,
    type DocumentNodeView as DocumentNode,
    type NoteDocumentView as NoteDocument,
  } from "./artifact";
  import DocumentView from "./DocumentView.svelte";
  let {
    document,
    node,
    onlink,
    card = false,
    paragraphs = [],
    more,
  }: {
    document?: NoteDocument | null;
    node?: DocumentNode;
    /** Links open through a native intent; chrome never navigates itself. */
    onlink?: (href: string) => void;
    /** The result card: the lead and the next steps, never the whole text. */
    card?: boolean;
    /** Plain paragraphs, for a card whose document carries no formatting. */
    paragraphs?: readonly string[];
    more?: (count: number) => string;
  } = $props();
  const STEPS = 3;
  const digest = $derived(card ? documentDigest({ paragraphs, formatted: document }) : null);
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

{#if digest}
  <div class="digest">
    {#if digest.lead}<p class="lead">{digest.lead}</p>{/if}
    {#if digest.steps.length}<p class="steps-label">{digest.stepsLabel}</p>
      <ul class="steps">
        {#each digest.steps.slice(0, STEPS) as step, index (index)}<li>
            <span class="tick" aria-hidden="true"></span><span class="step">{step}</span>
          </li>{/each}
      </ul>
      {#if digest.steps.length > STEPS && more}<p class="more">
          {more(digest.steps.length - STEPS)}
        </p>{/if}{/if}
  </div>
{:else if root}
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
  .digest {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-block-size: 0;
  }

  .lead {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    margin: 0;
    overflow: hidden;
    font-size: var(--text-label);
    line-height: 16px;
    text-wrap: pretty;
  }

  .steps-label {
    margin: 4px 0 0;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 600;
    line-height: 13px;
  }

  .steps {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .steps li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
    font-size: var(--text-label);
    line-height: 16px;
  }

  .tick {
    flex: none;
    inline-size: 10px;
    block-size: 10px;
    border-radius: 3px;
    box-shadow: inset 0 0 0 1.5px var(--color-border-strong);
  }

  .step {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .more {
    margin: 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 13px;
  }

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
