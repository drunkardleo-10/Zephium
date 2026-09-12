<script lang="ts">
  import { Folder01Icon, ArrowRight01Icon } from "@hugeicons/core-free-icons";
  import Icon from "$shared/ui/Icon";
  let {
    name,
    depth,
    expanded = true,
    ontoggle,
    class: className = "",
  }: {
    name: string;
    depth: number;
    expanded?: boolean;
    ontoggle?: () => void;
    class?: string;
  } = $props();
</script>

<li class={className}>
  <button
    type="button"
    class="folder-row"
    style:padding-inline-start={`${8 + Math.min(depth, 8) * 13}px`}
    aria-expanded={expanded}
    onclick={ontoggle}
    ><Icon icon={Folder01Icon} size={15} /><span>{name}</span><span
      class="folder-chevron"
      data-expanded={expanded}><Icon icon={ArrowRight01Icon} size={12} /></span
    ></button
  >
</li>
