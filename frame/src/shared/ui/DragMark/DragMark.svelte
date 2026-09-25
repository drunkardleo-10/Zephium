<script lang="ts">
  import { Globe02Icon } from "@hugeicons/core-free-icons";
  import FavIcon from "../FavIcon/FavIcon.svelte";

  let {
    x,
    y,
    image,
    tone = "mid",
  }: {
    x: number;
    y: number;
    image: ImageData | null;
    tone?: "dark" | "light" | "mid";
  } = $props();
</script>

<!--
  What follows the pointer at rail width, where a title has nowhere to go:
  the site's own mark on a small raised disc, just clear of the cursor.
-->
<div class="drag-mark" style:left={`${x + 10}px`} style:top={`${y + 6}px`} aria-hidden="true">
  <FavIcon {image} {tone} size={16} lit fallback={Globe02Icon} />
</div>

<style>
  .drag-mark {
    position: fixed;
    z-index: 50;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: var(--radius-capsule);
    background: var(--color-raised);
    box-shadow: var(--shadow-float);
    pointer-events: none;
  }
</style>
