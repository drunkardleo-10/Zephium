<script lang="ts">
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "./Icon.svelte";

  let {
    icon,
    label,
    size = 16,
    buttonSize = 28,
    active = false,
    disabled = false,
    haspopup = false,
    expanded,
    controls,
    class: className = "",
    onclick,
  }: {
    icon: IconSvgElement;
    label: string;
    size?: number;
    buttonSize?: number;
    active?: boolean;
    disabled?: boolean;
    haspopup?: boolean | "menu" | "dialog";
    expanded?: boolean;
    controls?: string;
    class?: string;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<button
  type="button"
  aria-label={label}
  title={label}
  aria-pressed={active || undefined}
  aria-haspopup={haspopup === true ? "menu" : haspopup || undefined}
  aria-expanded={haspopup ? expanded : undefined}
  aria-controls={controls}
  {disabled}
  {onclick}
  class={["icon-button", active && "icon-button-active", className]}
  style:--icon-button-size={`${buttonSize}px`}
>
  <Icon {icon} {size} />
</button>
