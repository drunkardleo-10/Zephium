<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { commands } from "$shared/ipc/bindings";
  import { surface } from "$domain/surface";

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
<div class="mode-rail" data-work={inWork} data-standalone={standalone}>
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
</div>

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

  .mode-trigger-rail {
    height: 26px;
    padding-inline: 9px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: 13px;
    font-weight: 550;
    letter-spacing: -0.006em;
    white-space: nowrap;
    cursor: default;
    outline: none;
    transition:
      background-color var(--motion-fast) var(--ease-out-quiet),
      color var(--motion-fast) var(--ease-out-quiet);
  }

  .mode-trigger-rail:hover {
    background: var(--row-hover);
    color: var(--color-text);
  }

  .mode-rail[data-work="true"] .mode-trigger-rail {
    color: var(--color-accent);
  }
</style>
