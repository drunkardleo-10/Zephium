<script lang="ts">
  import { ComputerTerminal01Icon } from "../../lib/icons";
  import CardFrame from "./CardFrame.svelte";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const command = $derived(item.command);
  const elapsed = (millis: number) =>
    millis < 1000
      ? m.work_timeline_ms({ ms: Math.max(1, Math.round(millis)) })
      : millis < 60_000
        ? m.work_timeline_seconds({ seconds: (millis / 1000).toFixed(1) })
        : m.work_timeline_minutes({ minutes: Math.round(millis / 60_000) });
  const state = $derived<"running" | "ok" | "failed" | null>(
    !command ? null : command.state === "running" ? "running" : command.exit ? "failed" : "ok",
  );
  /** How it ended, then how long it took; while it runs, how long so far. */
  const status = $derived.by(() => {
    if (!command) return item.status;
    if (command.state === "running")
      return command.elapsed_ms !== undefined
        ? m.work_card_command_running_for({ seconds: Math.floor(command.elapsed_ms / 1000) })
        : m.work_card_command_running();
    const exit = command.exit
      ? m.work_card_command_failed({ code: command.exit })
      : m.work_card_command_exit({ code: command.exit ?? 0 });
    return command.elapsed_ms ? `${exit} · ${elapsed(command.elapsed_ms)}` : exit;
  });
</script>

<CardFrame
  id={item.id}
  title={command?.line || item.title}
  icon={ComputerTerminal01Icon}
  {selected}
  active={command?.state === "running"}
  lines={1}
  mono
  dense
  row
>
  {#if command?.tail.length}<pre class="tail">{command.tail.slice(-3).join("\n")}</pre>{/if}
  {#snippet footer()}<span class="state" data-state={state}
      ><span class="dot" aria-hidden="true"></span>{status}</span
    >{#if command?.reason}<span class="reason">{command.reason}</span>{/if}{/snippet}
</CardFrame>

<style>
  .tail {
    margin: 0;
    padding-inline: 8px;
    overflow: hidden;
    color: var(--color-muted);
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    line-height: 13px;
    white-space: pre;
    text-overflow: ellipsis;
  }

  .state {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .dot {
    inline-size: 6px;
    block-size: 6px;
    border-radius: 50%;
    background: var(--color-faint);
  }

  .state[data-state="ok"] .dot {
    background: var(--color-success);
  }

  .state[data-state="failed"] {
    color: var(--color-danger);
  }

  .state[data-state="failed"] .dot {
    background: var(--color-danger);
  }

  .reason {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
