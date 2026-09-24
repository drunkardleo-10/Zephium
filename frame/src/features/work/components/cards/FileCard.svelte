<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { Doc01Icon, File01Icon, Image01Icon, SourceCodeIcon } from "../../lib/icons";
  import { homePath } from "../../lib/work-files";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const CODE = new Set(
    "c cc cpp cs css go h hpp html java js jsx json kt lua m mjs php py rb rs scss sh sql svelte swift toml ts tsx vue xml yaml yml zsh fish".split(
      " ",
    ),
  );
  const TEXT = new Set("csv doc docx log md markdown pdf rtf tex txt".split(" "));
  const IMAGE = new Set("avif bmp gif heic jpeg jpg png svg tif tiff webp".split(" "));
  const file = $derived(item.file);
  const name = $derived(file?.name || item.title);
  const icon = $derived.by(() => {
    const extension = name.includes(".") ? name.split(".").at(-1)!.toLowerCase() : "";
    if (CODE.has(extension)) return SourceCodeIcon;
    if (TEXT.has(extension)) return Doc01Icon;
    if (IMAGE.has(extension)) return Image01Icon;
    return File01Icon;
  });
  const what = $derived.by(() => {
    switch (file?.what) {
      case "read":
        return m.work_card_file_read();
      case "searched":
        return m.work_card_file_searched();
      case "changed":
        return m.work_card_file_changed();
      case "created":
        return m.work_card_file_created();
      default:
        return item.status;
    }
  });
  const folder = $derived(homePath(file?.folder ?? item.detail));
</script>

<CardFrame title={name} {icon} {selected} unavailable={item.unavailable} lines={1} dense>
  {#snippet footer()}<span class="folder" title={folder}><bdi>{folder}</bdi></span><span
      class="what"
      >{what}{#if file?.delta}<span class="delta"
          ><span class="plus">+{file.delta.plus}</span>
          <span class="minus">−{file.delta.minus}</span></span
        >{/if}</span
    >{/snippet}
</CardFrame>

<style>
  .folder {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: start;
  }

  .what {
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
    flex: none;
    color: var(--color-muted);
  }

  .delta {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }

  .plus {
    color: var(--color-success);
  }

  .minus {
    color: var(--color-danger);
  }
</style>
