<script lang="ts">
  import LazyView from "$shared/ui/LazyView";
  import { loadCodeBlock } from "../Code";
  import type { DocumentNodeView as Block } from "./artifact";
  import * as m from "$shared/i18n/messages";
  let {
    blocks,
    label,
    page = false,
  }: {
    blocks: readonly Block[];
    /** Names each code block for assistive technology. */
    label: string;
    /** The lift's reading page: larger headings, and Copy on each code block. */
    page?: boolean;
  } = $props();
  const marked = (node: Block, type: string) => node.marks?.some((mark) => mark.type === type);
  const code = (node: Block) => (node.content ?? []).map((child) => child.text ?? "").join("");
  let copied = $state<number | null>(null);
  let failed = $state(false);
  // Code blocks are numbered in reading order so each keeps its own Copy state.
  const fences = $derived.by(() => {
    const found: Block[] = [];
    const walk = (nodes: readonly Block[]) => {
      for (const node of nodes)
        if (node.type === "codeBlock") found.push(node);
        else walk(node.content ?? []);
    };
    walk(blocks);
    return found;
  });
  function copy(node: Block) {
    const index = fences.indexOf(node);
    void navigator.clipboard.writeText(code(node)).then(
      () => {
        copied = index;
        failed = false;
      },
      () => (failed = true),
    );
  }
</script>

{#snippet inline(
  nodes: readonly Block[],
)}{#each nodes as node, index (index)}{#if node.type === "hardBreak"}<br
      />{:else if marked(node, "code")}<code>{node.text}</code>{:else}<span
        class:bold={marked(node, "bold")}
        class:italic={marked(node, "italic")}>{node.text}</span
      >{/if}{/each}{/snippet}

{#snippet flow(nodes: readonly Block[])}
  {#each nodes as node, index (index)}
    {#if node.type === "paragraph"}<p>{@render inline(node.content ?? [])}</p>
    {:else if node.type === "heading"}
      {#if (node.attrs?.level ?? 2) <= 2}<h3>{@render inline(node.content ?? [])}</h3>
      {:else}<h4>{@render inline(node.content ?? [])}</h4>{/if}
    {:else if node.type === "bulletList" || node.type === "orderedList"}
      <svelte:element
        this={node.type === "orderedList" ? "ol" : "ul"}
        start={node.type === "orderedList" ? (node.attrs?.start ?? 1) : undefined}
      >
        {#each node.content ?? [] as item, at (at)}<li>
            {@render flow(item.content ?? [])}
          </li>{/each}
      </svelte:element>
    {:else if node.type === "blockquote"}<blockquote>{@render flow(node.content ?? [])}</blockquote>
    {:else if node.type === "codeBlock"}
      <div class="fence">
        <LazyView
          loader={loadCodeBlock}
          loadingLabel={m.surface_loading()}
          failureLabel={m.work_artifact_unavailable()}
          retryLabel={m.surface_retry()}
          >{#snippet children(CodeBlock)}<CodeBlock
              language={node.attrs?.language ?? "text"}
              text={code(node)}
              {label}
            />{/snippet}</LazyView
        >
        {#if page}<button type="button" class="copy" onclick={() => copy(node)}
            >{copied === fences.indexOf(node) ? m.work_code_copied() : m.work_code_copy()}</button
          >{/if}
      </div>
    {:else if node.type === "horizontalRule"}<hr />
    {/if}
  {/each}
{/snippet}

<div class="answer" class:page>
  {@render flow(blocks)}
  {#if failed}<p role="alert" class="failed">{m.work_code_copy_failed()}</p>{/if}
</div>

<style>
  /* One reply, set in the body size; the card and the page differ only in scale. */
  .answer {
    min-inline-size: 0;
    color: var(--color-text);
    font-size: var(--text-body);
    line-height: 20px;
    overflow-wrap: anywhere;
  }

  p,
  ul,
  ol,
  blockquote,
  .fence,
  hr {
    margin: 0 0 8px;
  }

  .answer > :last-child {
    margin-block-end: 0;
  }

  h3,
  h4 {
    margin: 12px 0 4px;
    font-size: var(--text-label);
    font-weight: 600;
    line-height: 16px;
  }

  h4 {
    color: var(--color-label-secondary);
  }

  .answer > :first-child {
    margin-block-start: 0;
  }

  ul,
  ol {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-inline-start: 18px;
  }

  li > :global(p),
  li > :global(ul),
  li > :global(ol) {
    margin-block-end: 0;
  }

  li > :global(ul),
  li > :global(ol) {
    margin-block-start: 6px;
  }

  li::marker {
    color: var(--color-faint);
  }

  blockquote {
    padding-inline-start: 10px;
    border-inline-start: 2px solid var(--color-border-strong);
    color: var(--color-muted);
  }

  code {
    padding: 1px 4px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    font-family: var(--font-mono);
    font-size: 0.92em;
  }

  .fence {
    position: relative;
    padding: 8px 10px;
    border-radius: var(--radius-row);
    background: var(--color-fill);
  }

  hr {
    border: 0;
    border-block-start: 1px solid var(--color-border);
  }

  .bold {
    font-weight: 600;
  }

  .italic {
    font-style: italic;
  }

  /* The reading page: a measure, a larger step between sections. */
  .page {
    line-height: 1.6;
  }

  .page p,
  .page ul,
  .page ol,
  .page blockquote,
  .page .fence {
    margin-block-end: 12px;
  }

  .page h3 {
    margin-block: 24px 8px;
    font-size: var(--text-page-title);
  }

  .page h4 {
    margin-block: 16px 6px;
    font-size: var(--text-body);
  }

  .copy {
    position: absolute;
    inset-block-start: 6px;
    inset-inline-end: 6px;
    padding: 2px 8px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-control);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
    opacity: 0;
    transition:
      opacity var(--motion-fast) var(--ease-out),
      background-color var(--motion-fast) var(--ease-out);
  }

  .copy:hover {
    background: var(--color-control-hover);
    color: var(--color-text);
  }

  .copy:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
    opacity: 1;
  }

  .fence:hover .copy {
    opacity: 1;
  }

  .failed {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  @media (prefers-reduced-motion: reduce) {
    .copy {
      transition: none;
    }
  }
</style>
