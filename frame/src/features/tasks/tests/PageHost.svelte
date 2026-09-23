<script lang="ts">
  import TasksPage from "../components/TasksPage.svelte";
</script>

<div class="stage">
  <div class="rail"></div>
  <TasksPage />
</div>

<style>
  .stage {
    display: flex;
    height: 760px;
    background: var(--color-chrome);
  }

  /* Stands in for the browser's tab rail, which stays beside the page. */
  .rail {
    flex: 0 0 56px;
  }

  .stage :global(.library-shell) {
    flex: 1;
    min-width: 0;
  }
</style>
