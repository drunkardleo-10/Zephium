<script lang="ts">
  import NotesPage from "../components/NotesPage.svelte";
  let { profile }: { profile: string } = $props();
</script>

<div class="stage">
  <div class="rail"></div>
  <div class="page"><NotesPage {profile} onclose={() => {}} /></div>
</div>

<style>
  .stage {
    display: flex;
    height: 820px;
    padding: 8px 8px 8px 0;
    box-sizing: border-box;
    background: var(--color-chrome);
  }

  /* Stands in for the browser's tab rail, which stays beside the page. */
  .rail {
    flex: 0 0 56px;
  }

  .page {
    flex: 1;
    min-width: 0;
  }
</style>
