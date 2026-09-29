<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import SubjectPicture from "./SubjectPicture.svelte";
  import HostGlyph from "./HostGlyph.svelte";
  import { displayHost } from "$shared/ui/data/Artifact";
  import PlayMark from "../objects/PlayMark.svelte";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected, onplay }: { item: CanvasItem; selected: boolean; onplay?: () => void } =
    $props();
  // A video is its poster with YouTube's own mark; it plays where every page
  // plays, in the Work browser, never in a player of the canvas's own.
  const playable = $derived(
    /^https?:\/\/([a-z0-9-]+\.)*(youtube\.com|youtu\.be)$/u.test(item.detail),
  );
  const host = $derived(displayHost(item.detail) || item.detail);
</script>

{#snippet thumbnail()}{#if playable}<button
      type="button"
      class="thumbnail poster nodrag nopan"
      aria-label={m.work_env_play_video({ title: item.title })}
      onclick={(event) => {
        event.stopPropagation();
        onplay?.();
      }}
    >
      <SubjectPicture picture={item.image} name={item.title} large />
      <span class="play-mark"><PlayMark youtube size={48} /></span>
    </button>{:else}<div class="thumbnail">
      <SubjectPicture picture={item.image} name={item.title} large />
    </div>{/if}{/snippet}

<CardFrame
  id={item.id}
  title={item.title}
  {selected}
  unavailable={item.unavailable}
  hero={item.image ? thumbnail : undefined}
  dense
>
  {#snippet leading()}<HostGlyph
      {host}
      url={item.detail}
      icon={item.icon ?? null}
      initial={false}
    />{/snippet}
  {#snippet footer()}<span class="host" title={item.detail}>{host || item.status}</span>{/snippet}
</CardFrame>

<style>
  .thumbnail {
    position: relative;
    aspect-ratio: 16 / 9;
  }

  .poster {
    display: block;
    inline-size: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: default;
  }

  .poster:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .play-mark {
    position: absolute;
    inset: 50% auto auto 50%;
    display: grid;
    translate: -50% -50%;
    opacity: 0.92;
    transition:
      opacity var(--motion-fast) var(--ease-out),
      scale var(--motion-base) var(--ease-spring);
  }

  .poster:hover .play-mark {
    opacity: 1;
    scale: 1.06;
  }

  .host {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
