<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  let {
    children,
    variant = "secondary",
    size = "regular",
    shape = "capsule",
    pending = false,
    disabled = false,
    ref = $bindable(),
    class: className = "",
    ...rest
  }: HTMLButtonAttributes & {
    children: Snippet;
    ref?: HTMLButtonElement;
    variant?: "primary" | "secondary" | "ghost" | "danger";
    size?: "compact" | "regular" | "large";
    shape?: "rounded" | "capsule";
    pending?: boolean;
  } = $props();
</script>

<button
  bind:this={ref}
  type="button"
  {...rest}
  disabled={disabled || pending}
  aria-busy={pending || undefined}
  class={["ui-button", className]}
  data-variant={variant}
  data-size={size}
  data-shape={shape}
>
  {#if pending}<span class="pending" aria-hidden="true"><i></i><i></i><i></i></span>{/if}
  {@render children()}
</button>

<style>
  /* A whisper of fill, a hairline ring and a one-pixel drop. State changes
     are flat color fades; the only movement is a 4% press. */
  .ui-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    box-sizing: border-box;
    min-height: var(--control-regular);
    padding: 0 14px;
    border: 0;
    border-radius: var(--radius-control);
    background: var(--color-control);
    color: var(--color-on-control);
    font: inherit;
    font-size: var(--text-body);
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
    box-shadow: var(--shadow-control);
    cursor: default;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    transition:
      background-color var(--motion-base) var(--ease-smooth),
      color var(--motion-base) var(--ease-smooth),
      box-shadow var(--motion-base) var(--ease-smooth),
      scale var(--motion-slow) var(--ease-smooth),
      opacity var(--motion-base) var(--ease-smooth);
  }

  .ui-button:disabled {
    opacity: 0.5;
  }

  .ui-button:hover:not(:disabled) {
    background: var(--color-control-hover);
    color: var(--color-on-control-strong);
  }

  .ui-button:active:not(:disabled) {
    background: var(--color-control-pressed);
    scale: 0.97;
    transition-duration: var(--motion-fast);
  }

  /* Primary is the tinted, layered button: the accent at 6% under a ring of
     the same accent. Graphite keeps it neutral; a space tint colors it. */
  .ui-button[data-variant="primary"] {
    background: color-mix(in srgb, var(--color-accent) 8%, transparent);
    color: var(--color-accent);
    box-shadow: var(--shadow-accent);
  }

  .ui-button[data-variant="primary"]:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    color: var(--color-accent);
  }

  .ui-button[data-variant="primary"]:active:not(:disabled) {
    background: color-mix(in srgb, var(--color-accent) 16%, transparent);
  }

  .ui-button[data-variant="ghost"] {
    background: transparent;
    color: var(--color-on-control);
    box-shadow: none;
  }

  .ui-button[data-variant="ghost"]:hover:not(:disabled) {
    background: var(--color-control-hover);
  }

  .ui-button[data-variant="ghost"]:active:not(:disabled) {
    background: var(--color-control-pressed);
  }

  .ui-button[data-variant="danger"] {
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    color: var(--color-danger);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-danger) 14%, transparent);
  }

  .ui-button[data-variant="danger"]:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-danger) 18%, transparent);
    color: var(--color-danger);
  }

  .ui-button[data-variant="danger"]:active:not(:disabled) {
    background: color-mix(in srgb, var(--color-danger) 24%, transparent);
  }

  .ui-button[data-size="compact"] {
    min-height: var(--control-compact);
    padding-inline: 10px;
    border-radius: var(--radius-control-compact);
    font-size: var(--text-label);
  }

  .ui-button[data-size="large"] {
    min-height: var(--control-large);
    padding-inline: 16px;
    border-radius: var(--radius-control-large);
  }

  .ui-button[data-shape="capsule"] {
    border-radius: var(--radius-capsule);
    padding-inline: 16px;
  }

  .ui-button[data-shape="capsule"][data-size="compact"] {
    padding-inline: 12px;
  }

  .pending {
    display: inline-flex;
    gap: 3px;
  }

  .pending > i {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: currentcolor;
    animation: pending 900ms var(--ease-in-out) infinite;
  }

  .pending > i:nth-child(2) {
    animation-delay: 150ms;
  }

  .pending > i:nth-child(3) {
    animation-delay: 300ms;
  }

  @keyframes pending {
    0%,
    100% {
      opacity: 0.25;
    }

    50% {
      opacity: 1;
    }
  }

  @media (forced-colors: active) {
    .ui-button {
      border: 1px solid ButtonText;
      box-shadow: none;
    }

    .ui-button[data-variant="primary"] {
      background: Highlight;
      color: HighlightText;
    }
  }
</style>
