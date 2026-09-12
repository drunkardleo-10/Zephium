<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import ChoiceGroup from "$shared/ui/ChoiceGroup";
  import { commands } from "$shared/ipc/bindings";
  let { compact = false }: { compact?: boolean } = $props();
</script>

{#if compact}<button
    type="button"
    class="compact-mode-word"
    aria-haspopup="menu"
    title={m.ui_mode_work_hint()}
    onclick={() => void commands.runCommand("mode.choose")}>{m.mode_work()}</button
  >
{:else}
  <div class="mode-footer">
    <ChoiceGroup
      label={m.mode_switch()}
      segmented
      full
      value="browse"
      options={[
        { value: "browse", label: m.mode_browse() },
        { value: "work", label: m.mode_work(), disabled: true },
      ]}
    />
  </div>
{/if}
