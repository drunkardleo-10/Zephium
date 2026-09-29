<script lang="ts">
  import type { ObjectActions, PageObjectView } from "../../lib/board/types";
  import PageFace from "../run/PageFace.svelte";
  /** A page as a small browser window: its site and title in a slim bar over its captured frame. */
  let { object, actions = {} }: { object: PageObjectView; actions?: ObjectActions } = $props();
</script>

<button
  type="button"
  class="page nodrag nopan"
  aria-label={object.title}
  onclick={() => (actions.open ? actions.open(object.id) : actions.link?.(object.url))}
>
  <PageFace
    url={object.url}
    title={object.title}
    frame={object.frame ?? null}
    live={!!object.live}
  />
</button>

<style>
  .page {
    display: block;
    box-sizing: border-box;
    inline-size: 100%;
    aspect-ratio: 336 / 238;
    padding: 0;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .page:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>
