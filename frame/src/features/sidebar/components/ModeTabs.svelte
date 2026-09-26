<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { Briefcase02Icon, Globe02Icon } from "@hugeicons/core-free-icons";
  import { surface } from "$domain/surface";
  import { preferences } from "$domain/preferences";
  import SegmentedControl from "$shared/ui/SegmentedControl";

  let {
    compact = false,
    standalone = false,
  }: {
    /** The rail's width: the same control, its glyphs alone. */
    compact?: boolean;
    /** Heads a rail beside a tool panel, with no header under it to end it. */
    standalone?: boolean;
  } = $props();

  let inWork = $derived(surface.currentPage() === "work");
  const options = [
    { value: "browse", label: m.mode_browse(), icon: Globe02Icon },
    { value: "work", label: m.mode_work(), icon: Briefcase02Icon },
  ];
</script>

<!--
  Two environments, one choice: the shared control, sized to the column. The
  binding reads the surface native has confirmed and turns a pick into a
  request, so the thumb only travels once the environment has actually
  changed and can never be left on a side native refused.
-->
{#if preferences.value("work.enabled") !== "false"}<div
    class="modes"
    data-compact={compact}
    data-standalone={standalone}
  >
    <SegmentedControl
      label={m.ui_mode_work_hint()}
      {options}
      full
      iconOnly={compact}
      size={compact ? "compact" : "regular"}
      bind:value={
        () => (inWork ? "work" : "browse"),
        (next) => void surface.open(next === "work" ? "work" : null)
      }
    />
  </div>{/if}

<style>
  .modes {
    display: flex;
    flex: 1;
    min-width: 0;
  }

  /* In the column the switch is one more band of it, so it takes the corner
     every band there shares rather than a free-standing control's. */
  .modes :global(.segmented[data-size="regular"]) {
    --seg-radius: var(--radius-row);
  }

  .modes[data-compact="true"] {
    flex: none;
    width: 48px;
  }

  .modes[data-compact="true"] :global(.segmented) {
    --seg-height: 26px;
  }

  /* Beside a tool panel the rail has no header to hold it off the window's
     edge or to end it, so it carries that space and that rule itself. */
  .modes[data-standalone="true"] {
    position: relative;
    align-self: center;
    margin-block: 8px 7px;
    padding-block-end: 7px;
  }

  .modes[data-standalone="true"]::after {
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
</style>
