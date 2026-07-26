<script lang="ts">
  import { Folder01Icon } from "@hugeicons/core-free-icons";
  import Icon from "../../ui/Icon.svelte";

  let {
    name,
    depth,
    class: className = "",
  }: {
    name: string;
    depth: number;
    class?: string;
  } = $props();
</script>

<li
  class={["flex h-[34px] items-center gap-2.5 rounded-md pe-2 text-[13px] text-muted", className]}
  style:padding-inline-start={`${8 + Math.min(depth, 8) * 13}px`}
>
  <Icon icon={Folder01Icon} size={15} class="shrink-0" />
  <span class="min-w-0 flex-1 truncate">{name}</span>
</li>
