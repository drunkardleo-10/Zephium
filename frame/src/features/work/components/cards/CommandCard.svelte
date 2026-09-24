<script lang="ts">
  import { ComputerTerminal01Icon } from "../../lib/icons";
  import CardFrame from "./CardFrame.svelte";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  /** Placeholder body until the local track's CommandRecord lands; routing stays in CanvasNode. */
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const command = $derived(item.command);
  const status = $derived.by(() => {
    if (!command) return item.status;
    if (command.state === "running")
      return command.elapsed_ms !== undefined
        ? m.work_card_command_running_for({ seconds: Math.floor(command.elapsed_ms / 1000) })
        : m.work_card_command_running();
    return command.exit === 0 || command.exit === undefined
      ? m.work_card_command_exit({ code: command.exit ?? 0 })
      : m.work_card_command_failed({ code: command.exit });
  });
  const failed = $derived(command?.state === "exit" && !!command.exit);
</script>

<CardFrame
  title={command?.line || item.title}
  icon={ComputerTerminal01Icon}
  {selected}
  active={command?.state === "running"}
  lines={1}
  mono
  dense
>
  {#if command?.tail.length}<pre class="tail">{command.tail.slice(-3).join("\n")}</pre>{/if}
  {#snippet footer()}<span class="state" class:failed>{status}</span>{#if command?.reason}<span
        class="reason">{command.reason}</span
      >{/if}{/snippet}
</CardFrame>

<style>
  .tail {
    margin: 0;
    overflow: hidden;
    color: var(--color-muted);
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: var(--text-caption);
    line-height: 13px;
    white-space: pre;
    text-overflow: ellipsis;
  }

  .state {
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .state.failed {
    color: var(--color-danger);
  }

  .reason {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
