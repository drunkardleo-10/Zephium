<script lang="ts">
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "../Icon/Icon.svelte";

  let {
    icon,
    label,
    size = 16,
    buttonSize = 28,
    variant = "ghost",
    active = false,
    disabled = false,
    haspopup = false,
    expanded,
    controls,
    class: className = "",
    onclick,
    pending = false,
    shape = "rounded",
  }: {
    icon: IconSvgElement;
    shape?: "rounded" | "circle";
    variant?: "ghost" | "glass";
    label: string;
    size?: number;
    buttonSize?: number;
    active?: boolean;
    disabled?: boolean;
    pending?: boolean;
    haspopup?: boolean | "menu" | "dialog";
    expanded?: boolean;
    controls?: string;
    class?: string;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<button
  type="button"
  data-shape={shape}
  data-variant={variant}
  aria-label={label}
  title={label}
  aria-pressed={active || undefined}
  aria-haspopup={haspopup === true ? "menu" : haspopup || undefined}
  aria-expanded={haspopup ? expanded : undefined}
  aria-controls={controls}
  disabled={disabled || pending}
  aria-busy={pending || undefined}
  {onclick}
  class={["icon-button", active && "icon-button-active", className]}
  style:--icon-button-size={`${buttonSize}px`}
>
  <Icon {icon} {size} />
</button>

<style>
  /* The raised sibling of the flat chrome button: same footprint, the shared
     control fill on top, and the same plate as every other active thing when
     it is the one that is on. */
  .icon-button[data-variant="glass"] {
    border-radius: var(--radius-control);
    background: var(--color-control);
    color: var(--color-on-control);
    box-shadow: var(--shadow-control);
    transition:
      background-color var(--motion-instant) var(--ease-smooth),
      color var(--motion-instant) var(--ease-smooth),
      box-shadow var(--motion-instant) var(--ease-smooth),
      scale var(--motion-slow) var(--ease-smooth);
  }

  .icon-button[data-variant="glass"][data-shape="circle"] {
    border-radius: var(--radius-capsule);
  }

  .icon-button[data-variant="glass"]:hover:not(:disabled) {
    background: var(--color-control-hover);
    color: var(--color-on-control-strong);
  }

  .icon-button[data-variant="glass"]:active:not(:disabled) {
    background: var(--color-control-pressed);
    transform: none;
    scale: 0.96;
    transition-duration: var(--motion-fast);
  }

  .icon-button[data-variant="glass"].icon-button-active {
    background: var(--row-active);
    box-shadow: var(--row-rim);
    color: var(--color-text);
  }

  @media (forced-colors: active) {
    .icon-button[data-variant="glass"] {
      border: 1px solid ButtonText;
      box-shadow: none;
    }
  }
</style>
