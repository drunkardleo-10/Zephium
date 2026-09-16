<script lang="ts">
  import FavIcon from "$shared/ui/FavIcon";
  import { favicons } from "$domain/favicons";
  let { origins, size = 17 }: { origins: string[]; size?: number } = $props();
</script>

<div class="strip">
  {#each origins as origin (origin)}
    {@const ref = { origin, revision: "a" }}
    <span data-origin={origin}>
      <FavIcon image={favicons.image(ref)} tone={favicons.tone(ref)} {size} lit />
    </span>
  {/each}
</div>

<style>
  .strip {
    display: flex;
    gap: 12px;
    padding: 12px;
    background: var(--color-chrome);
  }
</style>
