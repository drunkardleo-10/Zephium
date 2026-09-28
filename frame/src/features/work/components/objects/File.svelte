<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import CodeBlock from "$shared/ui/data/Code/CodeBlock.svelte";
  import type { Detail, FileView, ObjectActions } from "../../lib/board/types";
  import { File01Icon } from "./icons";
  import { size } from "./size-text";
  import { noteBlocks } from "./markdown";
  /**
   * A file shown as what it holds: an image is the image, a PDF its first
   * page, text and code a sheet with their first lines, anything else its own
   * icon with its name and kind.
   */
  let {
    object,
    detail,
    actions = {},
  }: { object: FileView; detail: Detail; actions?: ObjectActions } = $props();
  let failed = $state(false);
  const picture = $derived(object.picture && !failed ? object.picture : null);
  const markdown = $derived(
    /\.(md|markdown)$/iu.test(object.name) || object.language === "markdown",
  );
  const meta = $derived(
    [object.kindLabel, object.size ? size(object.size) : ""].filter(Boolean).join(" · "),
  );
</script>

<figure class="file {object.file} {detail}" aria-label={object.name}>
  <button
    type="button"
    class="face nodrag nopan"
    aria-label={object.name}
    onclick={() => actions.open?.(object.id)}
  >
    {#if (object.file === "image" || object.file === "pdf") && picture}
      <img
        class:sheet={object.file === "pdf"}
        src={picture.src}
        alt=""
        width={picture.width}
        height={picture.height}
        decoding="async"
        loading="lazy"
        draggable="false"
        onerror={() => (failed = true)}
      />
    {:else if (object.file === "text" || object.file === "code") && object.lines !== undefined}
      <span class="sheet lines" class:mono={object.file === "code"}>
        {#if detail === "full"}
          {#if object.file === "code"}<CodeBlock
              language={object.language ?? ""}
              text={object.lines}
              label={object.name}
              limit={12}
            />{:else if markdown}{#each noteBlocks(object.lines).slice(0, 8) as part, index (index)}{#if part.kind === "heading"}<strong
                  class="heading">{part.text}</strong
                >{:else if part.kind === "list"}{#each part.items as item, at (at)}<span
                    class="item">{item}</span
                  >{/each}{:else}<span class="para">{part.lines.join(" ")}</span
                >{/if}{/each}{:else}{object.lines.split("\n").slice(0, 14).join("\n")}{/if}
        {:else}{#each object.lines.split("\n").slice(0, 10) as line, index (index)}<span
              class="bar"
              style:inline-size={`${Math.min(100, (line.trim().length / 44) * 100)}%`}
            ></span>{/each}{/if}
      </span>
    {:else}
      <span class="icon">
        {#if picture}<img
            src={picture.src}
            alt=""
            width="64"
            height="64"
            decoding="async"
            onerror={() => (failed = true)}
          />{:else}<Icon icon={File01Icon} size={detail === "full" ? 40 : 80} />{/if}
      </span>
    {/if}
  </button>
  {#if detail !== "tile"}
    <figcaption>
      <span class="name">{object.name}</span>
      {#if meta && detail === "full"}<span class="meta">{meta}</span>{/if}
    </figcaption>
  {/if}
</figure>

<style>
  .file {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0;
    inline-size: 100%;
    color: var(--color-text);
  }

  .face {
    display: block;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  img {
    display: block;
    inline-size: 100%;
    block-size: auto;
    border-radius: var(--radius-card);
    box-shadow: var(--shadow-raised);
  }

  img.sheet {
    border-radius: var(--radius-inset);
    background: var(--color-raised);
    box-shadow: var(--shadow-float);
  }

  .lines {
    display: flex;
    flex-direction: column;
    gap: 2px;
    box-sizing: border-box;
    aspect-ratio: 1 / 1.1;
    padding: 20px 22px;
    overflow: hidden;
    border-radius: var(--radius-inset);
    background: var(--color-raised);
    box-shadow: var(--shadow-float);
    color: var(--color-label-secondary);
    font-size: var(--text-label);
    line-height: 1.55;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    mask-image: var(--mask-fade-bottom);
  }

  .heading {
    display: block;
    margin-block: 2px 4px;
    color: var(--color-text);
    font-size: var(--text-body);
    white-space: normal;
  }

  .para,
  .item {
    display: block;
    margin-block-end: 6px;
    white-space: normal;
  }

  .item::before {
    content: "•  ";
    color: var(--color-faint);
  }

  .lines.mono {
    font-family: var(--font-mono);
  }

  .bar {
    display: block;
    flex: none;
    block-size: 12px;
    margin-block: 7px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-strong);
  }

  .icon {
    display: grid;
    place-items: center;
    aspect-ratio: 4 / 3;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
    color: var(--color-muted);
  }

  .icon img {
    inline-size: 64px;
    block-size: 64px;
    border-radius: 0;
    box-shadow: none;
  }

  figcaption {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding-inline: 2px;
  }

  .name {
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 18px;
    overflow-wrap: anywhere;
  }

  .meta {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .overview .name {
    font-size: var(--text-overview-label);
    line-height: 1.3;
  }
</style>
