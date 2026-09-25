<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { commands } from "$shared/ipc/bindings";
  import { surface } from "$domain/surface";
  import { preferences } from "$domain/preferences";

  let {
    standalone = false,
  }: {
    /** True when this head has no header controls under it to terminate it. */
    standalone?: boolean;
  } = $props();

  let inWork = $derived(surface.currentPage() === "work");
</script>

<!--
  The rail's name for the environment it is in. There is no room beside a
  56px column to draw a panel, so the choice is handed to the native menu the
  app already builds for it.
-->
{#if preferences.value("work.enabled") !== "false"}<div
    class="mode-rail"
    data-work={inWork}
    data-standalone={standalone}
  >
    <button
      type="button"
      class="mode-trigger-rail"
      aria-label={m.ui_mode_work_hint()}
      title={m.ui_mode_work_hint()}
      aria-haspopup="menu"
      onclick={() => void commands.runCommand("mode.choose")}
    >
      {inWork ? m.mode_work() : m.mode_browse()}
    </button>
  </div>{/if}

<style>
  /*
    At rail width a glyph would be read as the first tab in the list, so the
    word survives instead, set small and tight enough that both names clear
    56px. A rule below it says where the head of the rail ends.
  */
  .mode-rail {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
  }

  /* Standing alone beside a tool panel, the head has no header above it to
     hold it off the window's edge, so it carries that space itself. */
  .mode-rail[data-standalone="true"] {
    margin-block: 8px 7px;
    padding-block-end: 7px;
  }

  .mode-rail[data-standalone="true"]::after {
    content: "";
    position: absolute;
    bottom: 0;
    left: 50%;
    width: 22px;
    height: 1px;
    border-radius: var(--radius-capsule);
    background: var(--color-border);
    translate: -50% 0;
  }

  /* The expanded switch, condensed: its own ground, carrying only the name
     of the side you are on, so at rail width it still reads as that switch
     rather than as loose text or one more tab. */
  .mode-trigger-rail {
    min-width: 44px;
    height: 22px;
    padding-inline: 7px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.005em;
    white-space: nowrap;
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      scale var(--motion-slow) var(--ease-spring);
  }

  .mode-trigger-rail:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .mode-trigger-rail:active {
    scale: 0.95;
    transition-duration: var(--motion-instant);
  }

  .mode-rail[data-work="true"] .mode-trigger-rail {
    color: var(--color-accent);
  }
</style>
