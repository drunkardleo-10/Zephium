<script lang="ts">
  import type { BrandArt } from "./index";

  let { mark, size = 32 }: { mark: BrandArt; size?: number } = $props();
</script>

<!-- First-party artwork shipped with the app, never a site's own favicon. A
     brand whose mark is a dark glyph carries a light one for a dark ground. -->
<span class="mark" style:--size={`${size}px`} aria-hidden="true"
  ><img class="light" src={mark.light} alt="" draggable="false" />{#if mark.dark}<img
      class="dark"
      src={mark.dark}
      alt=""
      draggable="false"
    />{/if}</span
>

<style>
  .mark {
    display: grid;
    inline-size: var(--size);
    block-size: var(--size);
  }

  /* Sized outright: a mark with only a view box has no intrinsic size and
     would otherwise stretch its grid track to the browser's default. */
  img {
    grid-area: 1 / 1;
    inline-size: var(--size);
    block-size: var(--size);
    object-fit: contain;
  }

  .dark {
    display: none;
  }

  :global(:root:not([data-theme="light"])) .mark:has(.dark) .light {
    display: none;
  }

  :global(:root:not([data-theme="light"])) .dark {
    display: block;
  }
</style>
