<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { Briefcase02Icon, Globe02Icon } from "@hugeicons/core-free-icons";
  import { surface } from "$domain/surface";
  import Icon from "$shared/ui/Icon";

  let inWork = $derived(surface.currentPage() === "work");
  const go = (work: boolean) => {
    if (work !== inWork) void surface.open(work ? "work" : null);
  };
</script>

<!--
  Two environments, one control. The lit half is a single object that travels
  rather than two fills that swap, so the switch reads as moving between
  places instead of repainting, and it takes the accent on the Work side
  because the window it lands in is no longer the browser's.
-->
<div class="modes" role="group" aria-label={m.ui_mode_work_hint()}>
  <span class="thumb" class:thumb-work={inWork} aria-hidden="true"></span>
  <button type="button" class="mode" aria-pressed={!inWork} onclick={() => go(false)}>
    <Icon icon={Globe02Icon} size={13} />
    <span>{m.mode_browse()}</span>
  </button>
  <button type="button" class="mode" aria-pressed={inWork} onclick={() => go(true)}>
    <Icon icon={Briefcase02Icon} size={13} />
    <span>{m.mode_work()}</span>
  </button>
</div>

<style>
  .modes {
    position: relative;
    display: grid;
    flex: 1;
    grid-template-columns: 1fr 1fr;
    min-width: 0;
    height: 30px;
    padding: 3px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
  }

  /* Sized against the track's padding box so one translation of its own
     width lands it exactly on the second half. */
  .thumb {
    position: absolute;
    inset: 3px auto 3px 3px;
    width: calc(50% - 3px);
    border-radius: var(--radius-capsule);
    background: var(--color-control);
    box-shadow: var(--shadow-control);
    transition:
      translate var(--motion-base) var(--ease-spring),
      scale var(--motion-fast) var(--ease-out),
      background-color var(--motion-base) var(--ease-smooth),
      box-shadow var(--motion-base) var(--ease-smooth);
  }

  .thumb-work {
    translate: 100% 0;
    background: var(--color-accent);
    box-shadow: none;
  }

  .modes:active .thumb {
    scale: 0.97;
  }

  .mode {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    min-width: 0;
    border: 0;
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: 12.5px;
    font-weight: 560;
    letter-spacing: -0.006em;
    white-space: nowrap;
    cursor: default;
    outline: none;
    transition: color var(--motion-base) var(--ease-smooth);
  }

  .mode:hover {
    color: var(--color-text);
  }

  .mode[aria-pressed="true"] {
    color: var(--color-on-control-strong);
  }

  /* The side you are not on keeps its glyph a step behind its label, so only
     the lit half presents as a whole object. */
  .mode[aria-pressed="false"] :global(svg) {
    opacity: 0.65;
  }

  .modes:has(.thumb-work) .mode[aria-pressed="true"] {
    color: var(--color-on-accent);
  }

  @media (forced-colors: active) {
    .mode[aria-pressed="true"] {
      border: 1px solid Highlight;
      border-radius: var(--radius-capsule);
    }
  }
</style>
