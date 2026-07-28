<script lang="ts">
  import { StarIcon } from "@hugeicons/core-free-icons";
  import Icon from "../../../shared/ui/Icon.svelte";

  let { dropping = false }: { dropping?: boolean } = $props();
</script>

<!--
  Essentials are profile-wide, so an empty grid would otherwise read as a bug.
  The placeholder doubles as the drop target's own affordance.
-->
<div
  class="flex flex-col items-center gap-1 rounded-lg border border-dashed px-3 py-4 text-center transition-[background-color,border-color] duration-[var(--motion-base)] ease-[var(--ease-out-quiet)]"
  class:border-border-strong={!dropping}
  class:border-accent={dropping}
  class:bg-fill={dropping}
>
  <span class="flex h-6 w-6 items-center justify-center rounded-full bg-fill text-faint">
    <Icon icon={StarIcon} size={13} />
  </span>
  <p class="text-[12.5px] font-medium text-text">Drag to add Essentials</p>
  <p class="text-[11.5px] leading-4 text-faint">Keep your most used sites one click away</p>
</div>
