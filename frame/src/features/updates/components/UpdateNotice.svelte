<!--
  Most launches have nothing to say about updates, so what draws a notice
  loads only once there is one.
-->
<script lang="ts">
  import * as notices from "../lib/notices.svelte";

  let { view, onabout = () => {} }: { view: "cards" | "glyph"; onabout?: () => void } = $props();

  let current = $derived(notices.current());
  let shown = $derived(current.card !== null || current.pill !== null);
  let Cards = $state.raw<typeof import("./UpdateCards.svelte").default | null>(null);
  let Glyph = $state.raw<typeof import("./UpdateGlyph.svelte").default | null>(null);

  // A view that failed to load leaves the column as it was.
  $effect(() => {
    if (!shown) return;
    if (view === "cards")
      void import("./UpdateCards.svelte").then(
        (module) => (Cards = module.default),
        () => {},
      );
    else
      void import("./UpdateGlyph.svelte").then(
        (module) => (Glyph = module.default),
        () => {},
      );
  });
</script>

{#if shown && view === "cards" && Cards}
  <Cards />
{:else if shown && view === "glyph" && Glyph}
  <Glyph {onabout} />
{/if}
