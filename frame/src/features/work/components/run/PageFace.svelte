<script lang="ts">
  import { getContext } from "svelte";
  import HostGlyph from "../cards/HostGlyph.svelte";
  import { Orb } from "$shared/ui/presence";
  import { canvasFar } from "../../lib/canvas-context";
  import { thumbnail } from "../../lib/frame-thumbs";

  /**
   * A page as a small browser window: a slim bar with the site's mark, its
   * host and the page's title over the frame the run captured. Without a
   * frame the page still reads as itself, its title set on its own sheet.
   */
  let {
    url,
    title,
    frame,
    host = "",
    live = false,
  }: {
    url: string;
    title: string;
    frame: string | null;
    host?: string;
    /** The agent is on this page now: its window is ringed and its frame follows. */
    live?: boolean;
  } = $props();

  /** From far away a settled page draws a small copy of its frame; the live one stays itself. */
  const scale = getContext<{ readonly far: boolean } | undefined>(canvasFar);
  const small = $derived(!!scale?.far && !live);
  /** A frame that failed to load, or came back one flat colour, is not shown. */
  let failed = $state<string | null>(null);
  let blank = $state<string | null>(null);
  const shown = $derived(!!frame && failed !== frame && blank !== frame);
  const site = $derived.by(() => {
    try {
      return new URL(url).hostname.replace(/^www\./u, "");
    } catch {
      return host.replace(/^www\./u, "");
    }
  });
  /** The title without the site's own name trailing it: the bar already says the site. */
  const heading = $derived.by(() => {
    const segments = title.split(/\s+[|·–—-]\s+/u).map((segment) => segment.trim());
    const own = segments.filter(Boolean);
    return own.length > 1 ? own.slice(0, -1).join(" – ") : title.trim();
  });

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

<span class="window" class:live>
  <span class="bar">
    <span class="mark"
      >{#if live}<Orb size={14} />{:else}<HostGlyph
          {host}
          {url}
          size={14}
          initial={false}
        />{/if}</span
    >
    <span class="host">{site}</span>
    <span class="heading">{heading}</span>
    {#if live}{#key frame}<span class="arrived" aria-hidden="true"></span>{/key}{/if}
  </span>
  <span class="view">
    {#if shown && small}
      <canvas class="thumb" use:thumbnail={{ url: frame!, width: 224 }}></canvas>
    {:else if shown}
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
        <span class="sign"><HostGlyph {host} {url} size={28} initial={false} /></span>
        <span class="words">{heading || site}</span>
      </span>
    {/if}
  </span>
</span>

<style>
  .window {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    overflow: hidden;
    border-radius: var(--radius-row);
    background: var(--color-surface);
    box-shadow:
      0 0 0 1px var(--color-border),
      var(--shadow-raised);
    transition: box-shadow var(--motion-base) var(--ease-out);
  }

  .window.live {
    box-shadow:
      0 0 0 1.5px var(--color-accent),
      var(--shadow-float);
  }

  .bar {
    position: relative;
    display: grid;
    flex: none;
    grid-template-columns: 14px max-content minmax(0, 1fr);
    align-items: center;
    gap: 7px;
    block-size: 28px;
    padding: 0 10px;
    border-block-end: 1px solid var(--color-border);
    background: var(--color-raised);
    font-size: var(--text-caption);
    line-height: 14px;
  }

  /* A new look at the live page draws one hairline across the bar, once: the
     page moved, and nothing moves between looks. */
  .arrived {
    position: absolute;
    inset: auto 0 -1px;
    block-size: 1.5px;
    background: var(--color-accent);
    transform-origin: left;
    animation: arrived var(--motion-page) var(--ease-emphasized) forwards;
  }

  @keyframes arrived {
    0% {
      scale: 0 1;
      opacity: 1;
    }

    70% {
      scale: 1 1;
      opacity: 1;
    }

    100% {
      scale: 1 1;
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .arrived {
      animation: none;
      opacity: 0;
    }
  }

  .mark {
    display: grid;
    place-items: center;
    inline-size: 14px;
    block-size: 14px;
  }

  .host {
    color: var(--color-text);
    font-weight: 600;
  }

  /* One line in the bar: a long title fades at the window's edge. */
  .heading {
    overflow: hidden;
    color: var(--color-muted);
    white-space: nowrap;
    mask-image: linear-gradient(to right, black calc(100% - 20px), transparent);
  }

  .view {
    position: relative;
    display: block;
    flex: 1;
    min-block-size: 0;
    overflow: hidden;
    background: var(--color-surface);
  }

  img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
    object-position: top;
    pointer-events: none;
  }

  .thumb {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
    object-position: top;
  }

  /* No picture of the page: its title, set, on the page's own sheet. */
  .face {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    padding: 18px 20px 20px;
  }

  .sign {
    display: grid;
    inline-size: 28px;
    block-size: 28px;
  }

  .words {
    display: -webkit-box;
    overflow: hidden;
    color: var(--color-text);
    font-size: 18px;
    font-weight: 600;
    letter-spacing: -0.015em;
    line-height: 23px;
    text-wrap: balance;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
  }
</style>
