<script module lang="ts">
  /** The one video playing on the canvas; starting another stops it. */
  let playing = $state<string | null>(null);
</script>

<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import type { Detail, MediaView, ObjectActions } from "../../lib/board/types";
  import { youtubeId } from "../../lib/link-media";
  import Mark from "./Mark.svelte";
  import PlayMark from "./PlayMark.svelte";
  import { watchNear } from "./near";
  /**
   * An image is the image. A video is its poster until played, then YouTube's
   * own player in place, created on play and gone once it is far from view or
   * another video starts. Audio is a compact player.
   */
  let {
    object,
    detail,
    actions = {},
  }: { object: MediaView; detail: Detail; actions?: ObjectActions } = $props();
  const video = $derived(object.media === "video" ? youtubeId(object.url) : null);
  const poster = $derived(object.poster ?? object.picture);
  let failed = $state(false);
  /** The player refused this context (YouTube's embed errors); the page plays it instead. */
  let refused = $state(false);
  const on = $derived(playing === object.id && !!video && !refused);
  let root = $state<HTMLElement>();
  let frame = $state<HTMLIFrameElement>();
  $effect(() =>
    root
      ? watchNear(root, (near) => {
          if (!near && playing === object.id) playing = null;
        })
      : undefined,
  );
  const origin = typeof location === "undefined" ? "" : location.origin;
  const source = $derived(
    video
      ? `https://www.youtube-nocookie.com/embed/${video}?autoplay=1&playsinline=1&rel=0&enablejsapi=1&origin=${encodeURIComponent(origin)}${object.startSecs ? `&start=${Math.floor(object.startSecs)}` : ""}`
      : "",
  );
  // The player speaks through postMessage once asked to listen: an error or silence
  // means it will not play here, and the video opens where every page opens.
  $effect(() => {
    if (!on) return;
    let ready = false;
    const listen = (event: MessageEvent) => {
      if (event.source !== frame?.contentWindow) return;
      let data: { event?: string; info?: unknown } | null;
      try {
        data = typeof event.data === "string" ? JSON.parse(event.data) : event.data;
      } catch {
        return;
      }
      if (data?.event === "onError") refused = true;
      else if (data?.event) ready = true;
    };
    window.addEventListener("message", listen);
    const hello = window.setInterval(() => {
      frame?.contentWindow?.postMessage(JSON.stringify({ event: "listening", id: object.id }), "*");
    }, 400);
    const silence = window.setTimeout(() => {
      if (!ready) refused = true;
    }, 9000);
    return () => {
      window.removeEventListener("message", listen);
      window.clearInterval(hello);
      window.clearTimeout(silence);
    };
  });
  $effect(() => {
    if (refused && playing === object.id) playing = null;
  });
  function play() {
    if (video && !refused) playing = object.id;
    else actions.link?.(object.url);
  }
</script>

<figure class="media {object.media} {detail}" bind:this={root} aria-label={object.title}>
  {#if object.media === "image"}
    {#if poster && !failed}<img
        class="image"
        src={poster.src}
        alt={object.title ?? ""}
        width={poster.width}
        height={poster.height}
        decoding="async"
        loading="lazy"
        draggable="false"
        onerror={() => (failed = true)}
      />{:else}<div class="missing">{object.title ?? m.work_media_missing()}</div>{/if}
  {:else if object.media === "video"}
    <div class="screen">
      {#if on}<iframe
          bind:this={frame}
          src={source}
          title={object.title ?? ""}
          allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
          referrerpolicy="strict-origin-when-cross-origin"
          sandbox="allow-scripts allow-same-origin allow-presentation"
          allowfullscreen
        ></iframe>
      {:else}
        {#if poster && !failed}<img
            src={poster.src}
            alt=""
            width={poster.width}
            height={poster.height}
            decoding="async"
            loading="lazy"
            draggable="false"
            onerror={() => (failed = true)}
          />{/if}
        {#if detail !== "tile"}<button
            type="button"
            class="play nodrag nopan"
            aria-label={m.work_media_play({ title: object.title ?? "" })}
            onclick={play}><PlayMark size={detail === "full" ? 52 : 96} /></button
          >{/if}
        {#if object.duration && detail !== "tile"}<span class="length">{object.duration}</span>{/if}
      {/if}
    </div>
    {#if detail !== "tile" && object.title}
      <figcaption>
        {#if video}<Mark address="youtube.com" size={detail === "full" ? 16 : 32} />{/if}
        <span class="title">{object.title}</span>
      </figcaption>
    {/if}
  {:else}
    <div class="player">
      <button
        type="button"
        class="round nodrag nopan"
        aria-label={m.work_media_play({ title: object.title ?? "" })}
        onclick={() => actions.link?.(object.url)}
        ><PlayMark size={detail === "full" ? 36 : 64} /></button
      >
      <div class="words">
        <span class="title">{object.title ?? m.work_media_audio()}</span>
        {#if object.duration}<span class="length-text">{object.duration}</span>{/if}
      </div>
      {#if detail === "full"}<span class="wave" aria-hidden="true"
          >{#each Array.from({ length: 28 }, (_, index) => index) as index (index)}<span
              style:block-size={`${30 + ((index * 37) % 70)}%`}
            ></span>{/each}</span
        >{/if}
    </div>
  {/if}
</figure>

<style>
  .media {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0;
    inline-size: 100%;
    color: var(--color-text);
  }

  .image {
    display: block;
    inline-size: 100%;
    block-size: auto;
    border-radius: var(--radius-card);
    box-shadow: var(--shadow-raised);
  }

  .missing {
    display: grid;
    place-items: center;
    aspect-ratio: 4 / 3;
    padding: 24px;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    color: var(--color-muted);
    font-size: var(--text-page-title);
    text-align: center;
  }

  .screen {
    position: relative;
    aspect-ratio: 16 / 9;
    overflow: hidden;
    border-radius: var(--radius-card);
    background: var(--color-canvas);
    box-shadow: var(--shadow-raised);
  }

  .screen img,
  .screen iframe {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    border: 0;
    object-fit: cover;
  }

  .play {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 0;
    border: 0;
    background: none;
    cursor: default;
  }

  .play :global(svg) {
    transition: transform var(--motion-fast) var(--ease-out);
  }

  .play:hover :global(svg) {
    transform: scale(1.06);
  }

  .play:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -4px;
  }

  .length {
    position: absolute;
    inset-block-end: 10px;
    inset-inline-end: 10px;
    padding: 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-float);
    font-size: var(--text-caption);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  figcaption {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-inline: 2px;
  }

  .title {
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 18px;
    text-wrap: pretty;
  }

  .player {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 16px 10px 10px;
    border-radius: var(--radius-capsule);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
  }

  .round {
    display: grid;
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--radius-capsule);
    background: none;
    cursor: default;
  }

  .words {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-inline-size: 0;
  }

  .length-text {
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }

  .wave {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 2px;
    block-size: 24px;
    min-inline-size: 60px;
    margin-inline-start: 8px;
  }

  .wave span {
    flex: 1;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-strong);
  }

  .overview .title {
    font-size: var(--text-overview-label);
    line-height: 1.3;
  }

  .overview .length {
    padding: 4px 14px;
    font-size: var(--text-overview-label);
  }
</style>
