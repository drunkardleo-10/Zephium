<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { Briefcase02Icon, Globe02Icon } from "@hugeicons/core-free-icons";
  import Icon from "$shared/ui/Icon";
  import { surface } from "$domain/surface";

  let { standalone = false }: { standalone?: boolean } = $props();

  const inWork = $derived(surface.currentPage() === "work");
  const MODES = [
    { work: false, label: m.mode_browse, icon: Globe02Icon },
    { work: true, label: m.mode_work, icon: Briefcase02Icon },
  ];
</script>

<!--
  The rail's two environments as one small switch: a mark for each, a thumb
  under the one you are in that slides across once native has moved there,
  and a click on the other goes. Names come on hover and to assistive
  technology.
-->
<div
  class="mode-switch"
  data-work={inWork}
  data-standalone={standalone}
  role="group"
  aria-label={m.ui_mode_work_hint()}
>
  <span class="thumb" aria-hidden="true"></span>
  {#each MODES as mode (mode.work)}<button
      type="button"
      class="mode"
      aria-pressed={inWork === mode.work}
      aria-label={mode.label()}
      title={mode.label()}
      onclick={() => {
        if (inWork !== mode.work) void surface.open(mode.work ? "work" : null);
      }}><Icon icon={mode.icon} size={14} strokeWidth={1.7} /></button
    >{/each}
</div>

<style>
  .mode-switch {
    --cell: 22px;

    position: relative;
    display: grid;
    grid-template-columns: repeat(2, var(--cell));
    gap: 0;
    padding: 2px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    outline: 0.5px solid var(--color-border);
    outline-offset: -0.5px;
  }

  .thumb {
    position: absolute;
    inset-block: 2px;
    inset-inline-start: 2px;
    inline-size: var(--cell);
    border-radius: var(--radius-capsule);
    background: var(--color-float);
    box-shadow: var(--shadow-raised);
    transition: transform var(--motion-base) var(--ease-emphasized);
  }

  .mode-switch[data-work="true"] .thumb {
    transform: translateX(var(--cell));
  }

  .mode {
    position: relative;
    display: grid;
    place-items: center;
    inline-size: var(--cell);
    block-size: var(--cell);
    padding: 0;
    border: 0;
    border-radius: var(--radius-capsule);
    background: transparent;
    color: var(--color-faint);
    cursor: default;
    transition: color var(--motion-fast) var(--ease-out);
  }

  .mode:hover {
    color: var(--color-label-secondary);
  }

  .mode[aria-pressed="true"] {
    color: var(--color-text);
  }

  .mode:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  /* Beside a tool panel the rail has no header to hold it off the window's
     edge or to end it, so it carries that space itself. */
  .mode-switch[data-standalone="true"] {
    align-self: center;
    margin-block: 8px 14px;
  }

  @media (prefers-reduced-motion: reduce) {
    .thumb {
      transition: none;
    }
  }
</style>
