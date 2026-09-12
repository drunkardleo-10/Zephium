<script lang="ts">
  import { useSvelteFlow, Panel } from "@xyflow/svelte";
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";
  const flow = useSvelteFlow();
</script>

<Panel position="bottom-left"
  ><div class="controls" aria-label={m.work_canvas_controls()}>
    <Button
      size="compact"
      onclick={() => flow.zoomOut({ duration: 0 })}
      aria-label={m.work_zoom_out()}>−</Button
    >
    <Button
      size="compact"
      onclick={() => flow.zoomIn({ duration: 0 })}
      aria-label={m.work_zoom_in()}>+</Button
    >
    <Button size="compact" onclick={() => flow.fitView({ duration: 0, padding: 0.2 })}
      >{m.work_fit()}</Button
    >
  </div></Panel
>

<style>
  .controls {
    display: flex;
    gap: 6px;
    padding: 6px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface);
  }
</style>
