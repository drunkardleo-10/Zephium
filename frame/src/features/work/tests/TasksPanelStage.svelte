<script lang="ts">
  import TasksPanel from "../components/bar/TasksPanel.svelte";
  let { profile }: { profile: string } = $props();
</script>

<div class="stage"><div class="work-popover work-popover-wide"><TasksPanel {profile} /></div></div>

<style>
  .stage {
    display: flex;
    justify-content: center;
    padding: 40px;
    background: var(--color-canvas);
  }
</style>
