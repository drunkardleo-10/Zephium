<script lang="ts">
  import HistoryPage from "../components/HistoryPage.svelte";
</script>

<!-- Mirrors the browser stage: the library fills a bounded window rather than
     growing to its content, which is what makes the list scroll at all. -->
<div class="stage"><HistoryPage /></div>

<style>
  .stage {
    display: flex;
    height: 720px;
    padding-inline-start: 8px;
    background: var(--color-chrome);
  }

  .stage :global(.library-shell) {
    flex: 1;
    min-width: 0;
  }
</style>
