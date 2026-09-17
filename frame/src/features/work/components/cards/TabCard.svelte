<script lang="ts">
  import FavIcon from "$shared/ui/FavIcon";
  import Icon from "$shared/ui/Icon";
  import CardFrame from "./CardFrame.svelte";
  import { PlayIcon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  // The webview forbids frames, so a video reads as a link with a play mark and
  // plays where every page plays: in the pane.
  const playable = $derived(
    /^https?:\/\/([a-z0-9-]+\.)*(youtube\.com|youtu\.be)$/u.test(item.detail),
  );
</script>

<CardFrame
  kind={item.detail || item.kind}
  title={item.title}
  {selected}
  unavailable={item.unavailable}
  dense
>
  {#snippet leading()}<FavIcon favicon={item.favicon ?? null} />{/snippet}
  {#snippet footer()}<span>{item.status}</span>{#if playable}<span class="play"
        ><Icon icon={PlayIcon} size={11} />{m.work_env_play_here()}</span
      >{/if}{/snippet}
</CardFrame>

<style>
  .play {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    padding: 1px 8px 1px 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
  }
</style>
