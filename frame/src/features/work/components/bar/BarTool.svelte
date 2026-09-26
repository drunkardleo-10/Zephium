<script lang="ts">
  import { Popover } from "bits-ui";
  import type { Snippet } from "svelte";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "$shared/ui/Icon";

  let {
    icon,
    label,
    open = false,
    disabled = false,
    small = false,
    wide = false,
    content,
    onopenchange,
    onclick,
  }: {
    icon: IconSvgElement;
    label: string;
    open?: boolean;
    disabled?: boolean;
    /** The field's own controls: a size down from the bar's tools. */
    small?: boolean;
    wide?: boolean;
    /** What the tool opens; without one the tool acts on a click. */
    content?: Snippet;
    onopenchange?: (open: boolean) => void;
    onclick?: () => void;
  } = $props();
</script>

{#snippet face(props: Record<string, unknown>)}
  <button
    {...props}
    type="button"
    class="bar-tool"
    class:small
    aria-label={label}
    title={label}
    {disabled}
    onclick={content ? (props.onclick as (event: MouseEvent) => void) : onclick}
  >
    <Icon {icon} size={small ? 16 : 18} strokeWidth={1.6} />
  </button>
{/snippet}

{#if content}
  <Popover.Root {open} onOpenChange={(next) => onopenchange?.(next)}>
    <Popover.Trigger {disabled}>
      {#snippet child({ props })}{@render face(props)}{/snippet}
    </Popover.Trigger>
    <Popover.Content
      class={["work-popover", wide && "work-popover-wide"].filter(Boolean).join(" ")}
      side="top"
      align="start"
      sideOffset={12}
      collisionPadding={12}
      preventScroll={false}
      aria-label={label}
    >
      {@render content()}
    </Popover.Content>
  </Popover.Root>
{:else}{@render face({})}{/if}

<style>
  .bar-tool {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 36px;
    block-size: 36px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: var(--color-label-secondary);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      scale var(--motion-base) var(--ease-spring);
  }

  .bar-tool:disabled {
    color: var(--color-faint);
  }

  .bar-tool:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .bar-tool.small {
    inline-size: 30px;
    block-size: 30px;
    border-radius: var(--radius-capsule);
    color: var(--color-muted);
  }

  .bar-tool:hover:not(:disabled),
  .bar-tool[data-state="open"] {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .bar-tool:active:not(:disabled) {
    scale: 0.94;
    transition-duration: var(--motion-instant);
  }

  :global(.work-popover) {
    z-index: 40;
    box-sizing: border-box;
    inline-size: min(380px, calc(100vw - 24px));
    max-block-size: calc(100vh - 120px);
    overflow: auto;
    padding: 8px;
    border-radius: var(--radius-menu);
    background: var(--color-menu);
    backdrop-filter: blur(12px) saturate(1.2);
    box-shadow: var(--shadow-popover);
    color: var(--color-text);
    font-size: var(--text-body);
    outline: none;
    transform-origin: var(--bits-floating-transform-origin, bottom);
    animation: work-popover-in var(--motion-slow) var(--ease-smooth);
  }

  :global(.work-popover-wide) {
    inline-size: min(460px, calc(100vw - 24px));
  }

  :global(.work-popover[data-state="closed"]) {
    animation: work-popover-out var(--motion-fast) var(--ease-exit) forwards;
  }

  @keyframes work-popover-in {
    from {
      opacity: 0;
      transform: scale(0.96) translateY(4px);
    }
  }

  @keyframes work-popover-out {
    to {
      opacity: 0;
      transform: scale(0.99);
    }
  }

  @media (forced-colors: active) {
    :global(.work-popover) {
      border: 1px solid ButtonText;
      backdrop-filter: none;
    }
  }
</style>
