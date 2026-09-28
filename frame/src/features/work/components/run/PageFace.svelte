<script lang="ts">
  import HostGlyph from "../cards/HostGlyph.svelte";

  let {
    url,
    title,
    frame,
    host = "",
  }: { url: string; title: string; frame: string | null; host?: string } = $props();

  /** A frame that failed to load, or came back one flat colour, is not shown. */
  let failed = $state<string | null>(null);
  let blank = $state<string | null>(null);
  const shown = $derived(!!frame && failed !== frame && blank !== frame);

  /** One look at a 12×8 copy: a capture of a page that had not drawn yet is a single tone. */
  function inspect(image: HTMLImageElement) {
    const source = image.currentSrc || image.src;
    try {
      const canvas = document.createElement("canvas");
      canvas.width = 12;
      canvas.height = 8;
      const context = canvas.getContext("2d", { willReadFrequently: true });
      if (!context) return;
      context.drawImage(image, 0, 0, 12, 8);
      const data = context.getImageData(0, 0, 12, 8).data;
      let sum = 0;
      let square = 0;
      const count = data.length / 4;
      for (let index = 0; index < data.length; index += 4) {
        const tone = 0.2126 * data[index]! + 0.7152 * data[index + 1]! + 0.0722 * data[index + 2]!;
        sum += tone;
        square += tone * tone;
      }
      const mean = sum / count;
      if (source && Math.sqrt(Math.max(0, square / count - mean * mean)) < 2.5) blank = frame;
    } catch {
      // A frame from another origin can't be read back; it is shown as it is.
    }
  }
</script>

{#if shown}
  <img
    src={frame}
    alt=""
    draggable="false"
    decoding="async"
    loading="lazy"
    onload={(event) => inspect(event.currentTarget as HTMLImageElement)}
    onerror={() => (failed = frame)}
  />
{:else}
  <span class="face">
    <HostGlyph {host} {url} size={18} initial={false} />
    <span class="words">{title}</span>
  </span>
{/if}

<style>
  img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
    object-position: top;
    pointer-events: none;
  }

  /* No picture of the page: its title, set, on the page's own sheet. */
  .face {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    padding: 12px 14px 14px;
    background: var(--color-surface);
  }

  .words {
    display: -webkit-box;
    overflow: hidden;
    color: var(--color-text);
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 19px;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow-wrap: anywhere;
  }
</style>
