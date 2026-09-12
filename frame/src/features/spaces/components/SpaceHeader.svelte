<script lang="ts">
  import { Layers01Icon } from "@hugeicons/core-free-icons";
  import { tabs } from "$domain/tabs";
  import Icon from "$shared/ui/Icon";

  let spaces = $derived(tabs.spaces());
  let active = $derived(spaces.find((space) => space.id === tabs.activeSpaceId()) ?? null);
  let name = $derived(active?.name ?? "Space");
</script>

<!--
  The space labels the list beneath it, so it sits above that list rather than
  in the footer. Switching is native authority and has no command yet, so this
  stays a label until the domain can honour it.
-->
<div class="flex h-8 shrink-0 items-center gap-2 px-3" title={name}>
  <Icon icon={Layers01Icon} size={13} class="shrink-0 text-faint" />
  <h2 class="min-w-0 flex-1 truncate text-[12.5px] font-medium tracking-[0.01em] text-text">
    {name}
  </h2>
  {#if spaces.length > 1}
    <span class="flex shrink-0 items-center gap-1" aria-hidden="true">
      {#each spaces as space (space.id)}
        <span
          class="h-1 w-1 rounded-full transition-colors duration-[var(--motion-fast)]"
          class:bg-text={space.id === active?.id}
          class:bg-faint={space.id !== active?.id}
        ></span>
      {/each}
    </span>
  {/if}
</div>
