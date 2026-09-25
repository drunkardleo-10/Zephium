<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import CardFrame from "./CardFrame.svelte";
  import SubjectPicture from "./SubjectPicture.svelte";
  import HostGlyph from "./HostGlyph.svelte";
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
    background: var(--color-float);
    box-shadow: var(--shadow-raised);
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
    background: var(--color-control);
    color: var(--color-muted);
    font: inherit;
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .play:hover {
    background: var(--color-control-hover);
    color: var(--color-text);
  }

  .play:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }
</style>
