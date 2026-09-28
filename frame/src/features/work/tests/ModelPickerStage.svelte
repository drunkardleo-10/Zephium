<script lang="ts">
  import ModelPicker from "../components/bar/ModelPicker.svelte";

  let { profile }: { profile: string } = $props();
</script>

<!-- The picker where the bar's trail holds it: on the bar's sheet, at the canvas's bottom edge. -->
<div class="stage">
  <div class="sheet">
    <span class="words">Plan my trip to the next YC batch from Warsaw</span>
    <ModelPicker {profile} onsettings={() => {}} />
  </div>
</div>

<style>
  .stage {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    background: var(--color-canvas);
  }

  .sheet {
    display: flex;
    align-items: center;
    gap: 8px;
    box-sizing: border-box;
    inline-size: 700px;
    padding: 8px 8px 10px 16px;
    border-radius: var(--radius-panel) var(--radius-panel) 0 0;
    background: var(--color-menu);
    box-shadow: var(--shadow-sheet);
  }

  .words {
    flex: 1;
    color: var(--color-text);
    font-size: 14px;
    line-height: 20px;
  }
</style>
