<script lang="ts">
  let { on }: { on: boolean } = $props();
</script>

<!-- The leading column of a choice menu. It is always rendered so the column
     holds its width; only the glyph inside it comes and goes. -->
<span class="ui-menu-mark" aria-hidden="true">
  {#if on}
    <svg viewBox="0 0 12 12" width="12" height="12" fill="none" stroke="currentColor">
      <path
        d="M2 6.3l2.7 2.7L10 3.4"
        stroke-width="1.7"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
  {/if}
</span>
