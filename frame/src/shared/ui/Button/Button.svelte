<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  let {
    children,
    variant = "secondary",
    size = "regular",
    shape = "rounded",
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
  /* Fill, and only fill. The ring-and-top-light recipe this used to wear made
     the button read as an outline of a button: on a dark card the hairline was
     the loudest thing in it, and the label sat at 80% so it looked switched
     off. A button is the most actionable thing on a settings row and should be
     the most solid — same fill, height and shape as the select beside it, so
     the two read as one family. The only movement is the press. */
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
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
    cursor: default;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    transition:
      background-color var(--motion-instant) var(--ease-smooth),
      color var(--motion-instant) var(--ease-smooth),
      box-shadow var(--motion-instant) var(--ease-smooth),
      scale var(--motion-fast) var(--ease-smooth),
      opacity var(--motion-fast) var(--ease-smooth);
  }

  .ui-button:disabled {
    opacity: 0.5;
  }

  .ui-button:hover:not(:disabled) {
    background: var(--color-field-hover);
  }

  .ui-button:active:not(:disabled) {
    background: var(--color-fill-pressed);
    scale: 0.97;
    transition-duration: var(--motion-fast);
  }

  /* Primary is the lit rung, solid: in a product with no accent hue the thing
     you are meant to press is simply the brightest thing on the row. Tinting
     8% of an accent behind a ring said "secondary, but coloured", which is
     why nothing in here ever looked like the obvious action. */
  .ui-button[data-variant="primary"] {
    background: var(--color-lit);
    color: var(--color-on-lit);
  }

  .ui-button[data-variant="primary"]:hover:not(:disabled) {
    background: var(--color-lit-hover);
  }

  .ui-button[data-variant="primary"]:active:not(:disabled) {
    background: var(--color-lit);
  }

  .ui-button[data-variant="ghost"] {
    background: transparent;
    color: var(--color-muted);
  }

  .ui-button[data-variant="ghost"]:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .ui-button[data-variant="ghost"]:active:not(:disabled) {
    background: var(--color-fill-pressed);
  }

  .ui-button[data-variant="danger"] {
    background: color-mix(in srgb, var(--color-danger) 14%, transparent);
    color: var(--color-danger);
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

  /* Kept for the one place a pill is the right answer — a tag, a filter — and
     no longer the default, which is how every button in settings ended up a
     pill in a column of rounded rectangles. */
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
