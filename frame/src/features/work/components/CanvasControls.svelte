<script lang="ts">
  import { useSvelteFlow, Panel } from "@xyflow/svelte";
  import Button from "$shared/ui/Button";
  import IconButton from "$shared/ui/IconButton";
  import Cursor02Icon from "@hugeicons/core-free-icons/Cursor02Icon";
  import HandGrabIcon from "@hugeicons/core-free-icons/HandGrabIcon";
  import type { PointerTool } from "../lib/selection";
  import * as m from "$shared/i18n/messages";
  let {
    bottomInset = 0,
    tool = "hand",
    ontool,
  }: { bottomInset?: number; tool?: PointerTool; ontool?: (tool: PointerTool) => void } = $props();
  const flow = useSvelteFlow();
</script>

<Panel position="bottom-left"
  ><div class="stack">
    {#if ontool}<div class="controls tools" role="group" aria-label={m.work_canvas_tool()}>
        <IconButton
          icon={Cursor02Icon}
          label={m.work_canvas_tool_select()}
          size={14}
          active={tool === "select"}
          onclick={() => ontool("select")}
        /><IconButton
          icon={HandGrabIcon}
          label={m.work_canvas_tool_hand()}
          size={14}
          active={tool === "hand"}
          onclick={() => ontool("hand")}
        />
      </div>{/if}
    <div class="controls" aria-label={m.work_canvas_controls()}>
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
      <Button
        size="compact"
        onclick={() =>
          flow.fitView({
            duration: 0,
            padding:
              bottomInset > 0
                ? { top: "110px", bottom: `${bottomInset + 32}px`, left: "40px", right: "40px" }
                : 0.2,
          })}>{m.work_fit()}</Button
      >
    </div>
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

  .stack {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
  }

  .tools {
    gap: 2px;
    padding: 4px;
  }
</style>
