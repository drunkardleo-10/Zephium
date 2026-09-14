<script lang="ts">
  import type { Snippet } from "svelte";
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
  const id = $props.id();
  const ratio = $derived(total > 0 ? Math.min(1, done / total) : 0);
  const headline = $derived(
    needsDecision ? m.work_env_needs_you() : total > 0 ? `${done}/${total}` : "—",
  );
  const label = $derived(
    needsDecision
      ? m.work_env_decision()
      : active
        ? m.work_env_working()
        : total > 0 && done < total
          ? m.work_env_to_do()
          : m.work_env_tasks(),
  );
</script>

<div class="dock" class:open>
  <button
    type="button"
    class="capsule"
    class:active
    class:attention={needsDecision}
    aria-expanded={open}
    aria-controls={`${id}-panel`}
    aria-label={m.work_env_tasks_summary({ done, total })}
    onclick={() => onopenchange(!open)}
  >
    <span class="top">
      <span class="headline">{headline}</span>
      <span class="bar" aria-hidden="true"><i style:inline-size={`${ratio * 100}%`}></i></span>
    </span>
    <span class="texture" aria-hidden="true"></span>
    <span class="label">{label}</span>
  </button>
  {#if open && panel}
    <section id={`${id}-panel`} class="panel" aria-label={m.work_env_tasks()}>
      {@render panel()}
    </section>
  {/if}
</div>

<style>
  .dock {
    position: absolute;
    inset-inline-end: 14px;
    inset-block-start: 50%;
    display: flex;
    align-items: center;
    gap: 12px;
    translate: 0 -50%;
    pointer-events: none;
  }

  .capsule,
  .panel {
    pointer-events: auto;
  }

  .capsule {
    position: relative;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    align-items: center;
    box-sizing: border-box;
    inline-size: 64px;
    block-size: 176px;
    padding: 10px 6px 14px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-menu);
    backdrop-filter: blur(12px) saturate(1.15);
    box-shadow: var(--shadow-popover);
    color: var(--color-text);
    overflow: hidden;
    cursor: default;
    transition:
      scale var(--motion-base) var(--ease-spring),
      box-shadow var(--motion-base) var(--ease-smooth);
  }

  .capsule:hover {
    scale: 1.03;
  }

  .capsule:active {
    scale: 0.98;
  }

  .top {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    inline-size: 100%;
    padding: 6px 0 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
  }

  .headline {
    font-size: 15px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.01em;
  }

  .bar {
    display: block;
    inline-size: 34px;
    block-size: 3px;
    border-radius: 2px;
    background: var(--color-track);
    overflow: hidden;
  }

  .bar i {
    display: block;
    block-size: 100%;
    border-radius: 2px;
    background: var(--color-text);
    transition: inline-size var(--motion-slow) var(--ease-smooth);
  }

  .texture {
    position: absolute;
    inset: 0;
    background-image: radial-gradient(circle, var(--color-border-strong) 1px, transparent 1.2px);
    background-size: 7px 7px;
    mask-image: linear-gradient(to bottom, transparent 32%, black 60%, transparent 96%);
    opacity: 0.55;
    transition: opacity var(--motion-slow) var(--ease-smooth);
  }

  .label {
    position: relative;
    z-index: 1;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--color-label-secondary);
  }

  .capsule.active {
    --capsule-accent: var(--color-warning);

    color: var(--color-text);
  }

  .capsule.active .bar i {
    background: var(--capsule-accent);
  }

  .capsule.active .texture {
    background-image: radial-gradient(circle, var(--capsule-accent) 1px, transparent 1.2px);
    opacity: 0.7;
    animation: capsule-breathe 2.8s var(--ease-in-out) infinite alternate;
  }

  .capsule.attention {
    --capsule-accent: var(--color-warning);
  }

  .capsule.attention .top {
    background: color-mix(in srgb, var(--color-warning) 22%, transparent);
  }

  @keyframes capsule-breathe {
    from {
      opacity: 0.45;
    }

    to {
      opacity: 0.85;
    }
  }

  .panel {
    box-sizing: border-box;
    inline-size: min(360px, calc(100vw - 120px));
    max-block-size: min(70vh, 640px);
    padding: 8px;
    border-radius: var(--radius-menu);
    background: var(--color-menu);
    backdrop-filter: blur(12px) saturate(1.2);
    box-shadow: var(--shadow-popover);
    overflow: auto;
    animation: capsule-panel-in var(--motion-slow) var(--ease-smooth);
  }

  @keyframes capsule-panel-in {
    from {
      opacity: 0;
      transform: translateX(8px) scale(0.98);
    }
  }

  .dock.open {
    flex-direction: row-reverse;
  }

  .capsule:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .capsule.active .texture {
      animation: none;
    }
  }
</style>
