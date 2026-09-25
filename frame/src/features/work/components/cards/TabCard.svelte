<script lang="ts">
  import FavIcon from "$shared/ui/FavIcon";
  import { favicons } from "$domain/favicons";
  import Icon from "$shared/ui/Icon";
  import CardFrame from "./CardFrame.svelte";
  import SubjectPicture from "./SubjectPicture.svelte";
  import { displayHost } from "$shared/ui/data/Artifact";
  import { PlayIcon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected, onplay }: { item: CanvasItem; selected: boolean; onplay?: () => void } =
    $props();
  // The webview forbids frames, so a video reads as a link with a play mark and
  // plays where every page plays: in the pane.
  const playable = $derived(
    /^https?:\/\/([a-z0-9-]+\.)*(youtube\.com|youtu\.be)$/u.test(item.detail),
  );
  const host = $derived(displayHost(item.detail) || item.detail);
</script>

{#snippet thumbnail()}<div class="thumbnail">
    <SubjectPicture picture={item.image} name={item.title} large />
    {#if playable}<span class="play-mark" aria-hidden="true"
        ><Icon icon={PlayIcon} size={14} /></span
      >{/if}
  </div>{/snippet}

<CardFrame
  title={item.title}
  {selected}
  unavailable={item.unavailable}
  hero={item.image ? thumbnail : undefined}
  dense
>
  {#snippet leading()}<FavIcon
      image={favicons.image(item.icon ?? null)}
      tone={favicons.tone(item.icon ?? null)}
    />{/snippet}
  {#snippet footer()}<span class="host" title={item.detail}>{host || item.status}</span
    >{#if playable}<button
        type="button"
        class="play nodrag nopan"
        onclick={(event) => {
          event.stopPropagation();
          onplay?.();
        }}><Icon icon={PlayIcon} size={11} />{m.work_env_play_here()}</button
      >{/if}{/snippet}
</CardFrame>

<style>
  .thumbnail {
    position: relative;
    aspect-ratio: 16 / 9;
  }

  .play-mark {
    position: absolute;
    inset: 50% auto auto 50%;
    display: grid;
    place-items: center;
    inline-size: 32px;
    block-size: 32px;
    border-radius: 50%;
    background: var(--color-menu);
    color: var(--color-text);
    translate: -50% -50%;
  }

  .host {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .play {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    padding: 1px 8px 1px 6px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      color var(--motion-fast) var(--ease-smooth);
  }

  .play:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .play:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }
</style>
