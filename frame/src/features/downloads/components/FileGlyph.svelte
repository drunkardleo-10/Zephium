<script lang="ts">
  import { File02Icon } from "@hugeicons/core-free-icons";
  import { downloadKind } from "$domain/downloads";
  import Icon from "$shared/ui/Icon";

  let { filename, size = 32 }: { filename: string; size?: number } = $props();

  // The extension itself says what the file is, and costs no artwork.
  let extension = $derived.by(() => {
    const dot = filename.lastIndexOf(".");
    const tail = dot > 0 ? filename.slice(dot + 1) : "";
    return tail.length > 0 && tail.length <= 4 ? tail.toUpperCase() : "";
  });
</script>

<span
  class="glyph"
  data-kind={downloadKind(filename)}
  style:--glyph={`${size}px`}
  aria-hidden="true"
  >{#if extension}{extension}{:else}<Icon
      icon={File02Icon}
      size={Math.round(size * 0.5)}
    />{/if}</span
>

<style>
  .glyph {
    --tint: var(--color-muted);

    display: grid;
    flex: none;
    place-items: center;
    width: var(--glyph);
    height: var(--glyph);
    border-radius: calc(var(--glyph) * 0.28);
    background: color-mix(in srgb, var(--tint) 14%, transparent);
    color: var(--tint);
    font-size: calc(var(--glyph) * 0.27);
    font-weight: 650;
    letter-spacing: 0.02em;
  }

  .glyph[data-kind="image"],
  .glyph[data-kind="video"] {
    --tint: var(--color-tint-sky);
  }

  .glyph[data-kind="audio"],
  .glyph[data-kind="pdf"] {
    --tint: var(--color-tint-rose);
  }

  .glyph[data-kind="archive"],
  .glyph[data-kind="app"] {
    --tint: var(--color-tint-sage);
  }
</style>
