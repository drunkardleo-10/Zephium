<script lang="ts">
  import LazyView from "$shared/ui/LazyView";
  import { loadCodeBlock } from "$shared/ui/data/Code";
  import type { DocumentNodeView as Node, EvidenceReference } from "$shared/ui/data/Artifact";
  import type { ProseBlock } from "../../lib/board/types";
  import Chips from "./Chips.svelte";
  import * as m from "$shared/i18n/messages";
  let {
    block,
    sources,
    onevidence,
  }: {
    block: ProseBlock;
    sources: Readonly<Record<string, EvidenceReference>>;
    onevidence?: (reference: EvidenceReference) => void;
  } = $props();
  const marked = (node: Node, type: string) => node.marks?.some((mark) => mark.type === type);
  const code = (node: Node) => (node.content ?? []).map((child) => child.text ?? "").join("");
  /** The first paragraph reads a step larger: it carries on from the board's lead. */
  const first = $derived(block.blocks[0]?.type === "paragraph" ? 0 : -1);
  const cites = (at: string) => block.cites[at] ?? [];
</script>

{#snippet inline(
  nodes: readonly Node[],
)}{#each nodes as node, index (index)}{#if node.type === "hardBreak"}<br
      />{:else if marked(node, "code")}<code>{node.text}</code>{:else}<span
        class:bold={marked(node, "bold")}
        class:italic={marked(node, "italic")}>{node.text}</span
      >{/if}{/each}{/snippet}

{#snippet cite(at: string)}{#if cites(at).length}<Chips
      keys={cites(at)}
      {sources}
      onopen={onevidence}
    />{/if}{/snippet}

{#snippet item(
  node: Node,
  at: string,
)}{#each node.content ?? [] as child, index (index)}{#if child.type === "paragraph"}<p>
        {@render inline(
          child.content ?? [],
        )}{#if index === (node.content?.length ?? 0) - 1 || node.content?.[index + 1]?.type !== "paragraph"}{@render cite(
            at,
          )}{/if}
      </p>{:else if child.type === "bulletList" || child.type === "orderedList"}{@render list(
        child,
        `${at}:${index}`,
      )}{/if}{/each}{/snippet}

{#snippet list(node: Node, at: string)}<svelte:element
    this={node.type === "orderedList" ? "ol" : "ul"}
    start={node.type === "orderedList" ? (node.attrs?.start ?? 1) : undefined}
    >{#each node.content ?? [] as child, row (row)}<li>
        {@render item(child, `${at}.${row}`)}
      </li>{/each}</svelte:element
  >{/snippet}

{#if block.state === "pending"}
  <div class="pending" role="status">
    <p class="label">{block.pending || m.work_board_pending()}</p>
    <span class="line"></span><span class="line"></span><span class="line short"></span>
  </div>
{:else}
  <div class="prose">
    {#each block.blocks as node, index (index)}
      {#if node.type === "paragraph"}<p class:lead={index === first}>
          {@render inline(node.content ?? [])}{@render cite(String(index))}
        </p>
      {:else if node.type === "heading"}
        {#if (node.attrs?.level ?? 2) <= 2}<h3>{@render inline(node.content ?? [])}</h3>
        {:else}<h4>{@render inline(node.content ?? [])}</h4>{/if}
      {:else if node.type === "bulletList" || node.type === "orderedList"}{@render list(
          node,
          String(index),
        )}
      {:else if node.type === "blockquote"}<blockquote>
          {#each node.content ?? [] as child, at (at)}<p>
              {@render inline(child.content ?? [])}
            </p>{/each}
          {@render cite(String(index))}
        </blockquote>
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
                label={block.title ?? ""}
              />{/snippet}</LazyView
          >
        </div>
      {:else if node.type === "horizontalRule"}<hr />
      {/if}
    {/each}
  </div>
{/if}

<style>
  /* The answer as reading text: a measure, a lead a step larger, air between sections. */
  .prose {
    max-inline-size: 72ch;
    color: var(--color-text);
    font-size: calc(var(--text-body) + 1px);
    line-height: 22px;
    overflow-wrap: anywhere;
    text-wrap: pretty;
  }

  p,
  ul,
  ol,
  blockquote,
  .fence,
  hr {
    margin: 0 0 12px;
  }

  .prose > :last-child {
    margin-block-end: 0;
  }

  .lead {
    color: var(--color-text);
    font-size: calc(var(--text-body) + 3px);
    line-height: 25px;
    letter-spacing: -0.006em;
  }

  h3,
  h4 {
    margin: 20px 0 6px;
    font-size: calc(var(--text-body) + 2px);
    font-weight: 600;
    line-height: 20px;
    letter-spacing: -0.005em;
  }

  h4 {
    font-size: var(--text-body);
    color: var(--color-label-secondary);
  }

  .prose > :first-child {
    margin-block-start: 0;
  }

  ul,
  ol {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-inline-start: 20px;
    list-style: disc;
  }

  ol {
    list-style: decimal;
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
    padding-inline-start: 12px;
    border-inline-start: 2px solid var(--color-border-strong);
    color: var(--color-muted);
  }

  code {
    padding: 1px 5px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    font-family: var(--font-mono);
    font-size: 0.88em;
  }

  .fence {
    padding: 10px 12px;
    border-radius: var(--radius-row);
    background: var(--color-fill);
    font-size: var(--text-label);
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

  .pending {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .label {
    margin: 0 0 4px;
    color: var(--color-muted);
    font-size: var(--text-body);
  }

  /* Skeleton lines exist only while the run is live: they breathe, then give way to the answer. */
  .line {
    block-size: 10px;
    inline-size: 100%;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    animation: breathe calc(var(--motion-page) * 3) var(--ease-in-out) infinite alternate;
  }

  .line.short {
    inline-size: 60%;
  }

  @keyframes breathe {
    from {
      opacity: 0.55;
    }

    to {
      opacity: 1;
    }
  }
</style>
