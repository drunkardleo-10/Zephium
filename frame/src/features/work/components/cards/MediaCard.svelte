<script lang="ts">
  import { mediaSize, mediaUrl } from "$domain/resources";
  import CardFrame from "./CardFrame.svelte";
  import { File01Icon, Image01Icon, Pdf01Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const asset = $derived(item.media?.asset);
  const url = $derived(
    item.media && asset?.kind === "image" ? mediaUrl(item.media.profile, asset.digest) : null,
  );
  let failed = $state(false);
  const icon = $derived(
    asset?.kind === "image" ? Image01Icon : asset?.kind === "pdf" ? Pdf01Icon : File01Icon,
  );
</script>

<CardFrame
  id={item.id}
  kind={item.kind}
  title={item.title}
  {icon}
  {selected}
  unavailable={item.unavailable}
>
  {#if url && !failed}
    <div class="preview">
      <img src={url} alt={item.title} loading="lazy" onerror={() => (failed = true)} />
    </div>
  {:else if asset}
    <div class="file">
      <span class="meta">{asset.mime}</span>
      {#if failed}<span class="missing">{m.work_media_missing()}</span>{/if}
    </div>
  {/if}
  {#snippet footer()}
    <span>{item.status}</span>
    {#if asset}<span>{mediaSize(asset.bytes)}</span>{/if}
  {/snippet}
</CardFrame>

<style>
  .preview {
    display: grid;
    place-items: center;
    block-size: 100%;
    min-block-size: 0;
    margin-block-end: 6px;
    border-radius: var(--radius-row);
    background: var(--color-fill);
    overflow: hidden;
  }

  .preview img {
    max-inline-size: 100%;
    max-block-size: 100%;
    object-fit: contain;
    display: block;
  }

  .file {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 0;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .missing {
    color: var(--color-warning);
  }
</style>
