<script lang="ts">
  import { Popover } from "bits-ui";
  import type { Snippet } from "svelte";
  import Icon from "$shared/ui/Icon";
  import { CheckmarkCircle02Icon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";
  let {
    done,
    total,
    active = false,
    needsDecision = false,
    open = false,
    panel,
    onopenchange,
  }: {
    done: number;
    total: number;
    active?: boolean;
    needsDecision?: boolean;
    open?: boolean;
    panel?: Snippet;
    onopenchange: (open: boolean) => void;
  } = $props();
  const ratio = $derived(total > 0 ? Math.min(1, done / total) : 0);
  const headline = $derived(
    needsDecision
      ? m.work_env_needs_you()
      : total > 0
        ? m.work_env_tasks_progress({ done, total })
        : m.work_env_tasks(),
  );
</script>

<!-- The tasks in one quiet pill beside the space's name; its panel opens under it. -->
<Popover.Root {open} onOpenChange={onopenchange}>
  <Popover.Trigger
    class="work-tasks-pill"
    data-attention={needsDecision || undefined}
    data-active={active || undefined}
    aria-label={m.work_env_tasks_summary({ done, total })}
    disabled={!panel}
  >
    <span class="glyph" aria-hidden="true"><Icon icon={CheckmarkCircle02Icon} size={14} /></span>
    <span class="headline">{headline}</span>
    {#if total > 0 && !needsDecision}<span class="bar" aria-hidden="true"
        ><i style:inline-size={`${ratio * 100}%`}></i></span
      >{/if}
  </Popover.Trigger>
  <Popover.Content
    class="work-popover work-tasks-panel"
    align="start"
    sideOffset={8}
    collisionPadding={12}
    preventScroll={false}
    aria-label={m.work_env_tasks()}
  >
    {@render panel?.()}
  </Popover.Content>
</Popover.Root>

<style>
  :global(.work-tasks-pill) {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    block-size: 26px;
    padding: 0 10px 0 7px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    white-space: nowrap;
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      color var(--motion-fast) var(--ease-smooth);
  }

  :global(.work-tasks-pill:hover),
  :global(.work-tasks-pill[data-state="open"]) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  :global(.work-tasks-pill:focus-visible) {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  :global(.work-tasks-pill[data-attention]) {
    background: color-mix(in srgb, var(--color-warning) 18%, transparent);
    color: var(--color-text);
  }

  .glyph {
    display: grid;
    place-items: center;
  }

  :global(.work-tasks-pill[data-attention]) .glyph,
  :global(.work-tasks-pill[data-active]) .glyph {
    color: var(--color-warning);
  }

  .headline {
    font-variant-numeric: tabular-nums;
  }

  .bar {
    display: block;
    inline-size: 48px;
    block-size: 3px;
    border-radius: var(--radius-capsule);
    background: var(--color-track);
    overflow: hidden;
  }

  .bar i {
    display: block;
    block-size: 100%;
    border-radius: inherit;
    background: var(--color-label-secondary);
    transition: inline-size var(--motion-slow) var(--ease-smooth);
  }

  :global(.work-tasks-panel) {
    inline-size: min(360px, calc(100vw - 24px));
    max-block-size: min(70vh, 640px);
  }
</style>
