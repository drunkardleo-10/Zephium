<script lang="ts">
  import { Character, Shimmer, type Mood } from "$shared/ui/presence";
  import type { PartView } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";

  /**
   * The helper that owns a part, standing at the part's top-right with one
   * line of what it does now: at work the line shimmers, done it rests.
   */
  let { part }: { part: Pick<PartView, "helper" | "state" | "now" | "summary"> } = $props();

  const DOING: Record<PartView["helper"], Mood> = {
    browser: "reading",
    research: "searching",
    computer: "working",
    connection: "working",
    lead: "reading",
  };
  const working = $derived(part.state === "running");
  const mood = $derived<Mood>(
    part.state === "waiting"
      ? "waiting"
      : working
        ? part.now?.startsWith(m.work_line_searching())
          ? "searching"
          : DOING[part.helper]
        : part.state === "done"
          ? "done"
          : "rest",
  );
  const said = $derived(
    part.state === "waiting"
      ? m.work_line_waiting_for_you()
      : working
        ? part.now || (part.helper === "research" ? m.work_line_searching() : m.work_env_working())
        : part.state === "done"
          ? m.work_part_state_done()
          : m.work_part_state_next(),
  );
</script>

<!-- A part that could not finish says so once, in its own row, with its one remedy: no second voice here. -->
{#if part.state !== "failed" && part.state !== "stopped"}
  <span class="part-agent {part.state}" title={said}>
    <span class="face"><Character kind={part.helper} {mood} size={16} /></span>
    <span class="said"><Shimmer text={said} running={working} /></span>
  </span>
{/if}

<style>
  .part-agent {
    display: flex;
    align-items: center;
    gap: 6px;
    max-inline-size: 100%;
    block-size: 20px;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
    pointer-events: none;
  }

  .part-agent.running {
    animation: agent-in var(--motion-slow) var(--ease-spring);
  }

  .face {
    display: grid;
    flex: none;
    place-items: center;
  }

  .said {
    min-inline-size: 0;
    overflow: hidden;
  }

  .running,
  .waiting {
    color: var(--color-text);
  }

  .waiting .said {
    font-weight: 600;
  }

  .done .said,
  .planned .said {
    color: var(--color-faint);
  }

  @keyframes agent-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
</style>
