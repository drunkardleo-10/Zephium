<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { surface } from "$domain/surface";

  let { standalone = false }: { standalone?: boolean } = $props();

  let open = $state(false);
  let root = $state<HTMLElement>();
  let other = $state<HTMLButtonElement>();
  const inWork = $derived(surface.currentPage() === "work");
  const current = $derived(inWork ? m.mode_work() : m.mode_browse());
  const next = $derived(inWork ? m.mode_browse() : m.mode_work());

  function choose() {
    open = false;
    void surface.open(inWork ? null : "work");
  }
</script>

<svelte:window
  onpointerdown={(event) => {
    if (open && event.target instanceof Node && !root?.contains(event.target)) open = false;
  }}
  onkeydown={(event) => {
    if (event.key !== "Escape" || !open) return;
    event.stopPropagation();
    open = false;
  }}
/>

<!--
  The rail names where you are in a word. Choosing it opens the other one
  just beneath, on the same plate, and the plate folds back once one is picked.
-->
<div
  class="mode-word"
  data-open={open}
  data-standalone={standalone}
  bind:this={root}
  role="group"
  aria-label={m.ui_mode_work_hint()}
>
  <span class="plate" aria-hidden="true"></span>
  <button
    type="button"
    class="word current"
    aria-expanded={open}
    aria-haspopup="true"
    onclick={() => (open = !open)}
    onkeydown={(event) => {
      if (event.key !== "ArrowDown") return;
      event.preventDefault();
      open = true;
      requestAnimationFrame(() => other?.focus());
    }}>{current}</button
  >
  <div class="drop" inert={!open}>
    <button type="button" class="word other" bind:this={other} onclick={choose}>{next}</button>
  </div>
</div>

<style>
  .mode-word {
    --row: 24px;

    position: relative;
    z-index: 3;
    display: flex;
    flex-direction: column;
    inline-size: 48px;
    block-size: var(--row);
  }

  .plate {
    position: absolute;
    inset: 0 0 auto;
    block-size: var(--row);
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
    box-shadow: none;
    transition:
      block-size var(--motion-base) var(--ease-emphasized),
      background-color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-base) var(--ease-out);
  }

  .mode-word:hover .plate {
    background: var(--color-fill-hover);
  }

  .mode-word[data-open="true"] .plate {
    block-size: calc(var(--row) * 2 + 2px);
    background: var(--color-float);
    box-shadow: var(--shadow-menu);
  }

  .word {
    position: relative;
    display: grid;
    inline-size: 100%;
    block-size: var(--row);
    padding: 0;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: -0.005em;
    place-items: center;
    cursor: default;
  }

  .current {
    color: var(--color-text);
  }

  .drop {
    position: absolute;
    inset-block-start: calc(var(--row) + 2px);
    inset-inline: 0;
    opacity: 0;
    translate: 0 -4px;
    transition:
      opacity var(--motion-fast) var(--ease-exit),
      translate var(--motion-fast) var(--ease-exit);
  }

  .mode-word[data-open="true"] .drop {
    opacity: 1;
    translate: 0 0;
    transition:
      opacity var(--motion-base) var(--ease-out) 40ms,
      translate var(--motion-base) var(--ease-emphasized) 40ms;
  }

  .other {
    color: var(--color-label-secondary);
  }

  .other:hover,
  .other:focus-visible {
    background: var(--row-active);
    color: var(--color-text);
  }

  .word:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  /* Beside a tool panel the rail has no header to hold it off the window's
     edge or to end it, so it carries that space and that rule itself. */
  .mode-word[data-standalone="true"] {
    align-self: center;
    margin-block: 8px 14px;
  }

  @media (prefers-reduced-motion: reduce) {
    .plate,
    .drop {
      transition: none;
    }
  }
</style>
