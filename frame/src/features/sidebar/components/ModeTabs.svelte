<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { Briefcase02Icon, Globe02Icon } from "@hugeicons/core-free-icons";
  import { surface } from "$domain/surface";
  import { preferences } from "$domain/preferences";
  import SegmentedControl from "$shared/ui/SegmentedControl";

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
{#if preferences.value("work.enabled") !== "false"}<div class="modes">
    <SegmentedControl
      label={m.ui_mode_work_hint()}
      {options}
      full
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
</style>
