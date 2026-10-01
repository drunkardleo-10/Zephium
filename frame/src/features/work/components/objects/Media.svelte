<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import type { MediaView, ObjectActions } from "../../lib/board/types";
  import { youtubeId } from "../../lib/link-media";
  import Mark from "./Mark.svelte";
  import PlayMark from "./PlayMark.svelte";
  /**
   * An image is the image. A video is its poster with a play mark (YouTube's
   * own on a YouTube video); a click opens its page where every page opens.
   * Audio is a compact player that opens its page the same way.
   */
  let { object, actions = {} }: { object: MediaView; actions?: ObjectActions } = $props();
  const youtube = $derived(object.media === "video" && !!youtubeId(object.url));
  const poster = $derived(object.poster ?? object.picture);
  /** The address that would not load; a new poster gets its own chance. */
  let broken = $state<string | null>(null);
  const failed = $derived(!!poster && broken === poster.src);
</script>

<figure class="media {object.media}" aria-label={object.title}>
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
        onerror={() => (broken = poster?.src ?? null)}
      />{:else}<div class="missing">{object.title ?? m.work_media_missing()}</div>{/if}
  {:else if object.media === "video"}
    <button
      type="button"
      class="screen nodrag nopan"
      aria-label={m.work_media_play({ title: object.title ?? "" })}
      onclick={() => actions.link?.(object.url)}
    >
      {#if poster && !failed}<img
          src={poster.src}
          alt=""
          width={poster.width}
          height={poster.height}
          decoding="async"
          loading="lazy"
          draggable="false"
          onerror={() => (broken = poster?.src ?? null)}
        />{/if}
      <span class="play"><PlayMark size={youtube ? 60 : 52} {youtube} /></span>
      {#if object.duration}<span class="length">{object.duration}</span>{/if}
    </button>
    {#if object.title}
      <figcaption>
        {#if youtube}<Mark address="youtube.com" size={16} />{/if}
        <span class="title">{object.title}</span>
      </figcaption>
    {/if}
  {:else}
    <div class="player">
      <button
        type="button"
        class="round nodrag nopan"
        aria-label={m.work_media_play({ title: object.title ?? "" })}
        onclick={() => actions.link?.(object.url)}><PlayMark size={36} /></button
      >
      <div class="words">
        <span class="title">{object.title ?? m.work_media_audio()}</span>
        {#if object.duration}<span class="length-text">{object.duration}</span>{/if}
      </div>
      <span class="wave" aria-hidden="true"
        >{#each Array.from({ length: 28 }, (_, index) => index) as index (index)}<span
            style:block-size={`${30 + ((index * 37) % 70)}%`}
          ></span>{/each}</span
      >
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
    display: block;
    aspect-ratio: 16 / 9;
    padding: 0;
    overflow: hidden;
    border: 0;
    border-radius: var(--radius-card);
    background: var(--color-canvas);
    box-shadow: var(--shadow-raised);
    color: inherit;
    font: inherit;
    cursor: default;
  }

  .screen:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .screen img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
  }

  .play {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }

  .play :global(svg) {
    transition: transform var(--motion-fast) var(--ease-out);
  }

  .screen:hover .play :global(svg) {
    transform: scale(1.06);
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
</style>
