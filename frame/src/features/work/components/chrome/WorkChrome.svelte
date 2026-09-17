<script lang="ts">
  import { Popover } from "bits-ui";
  import { untrack, type Snippet } from "svelte";
  import Icon from "$shared/ui/Icon";
  import {
    ArrowDown01Icon,
    ArrowLeft02Icon,
    Attachment01Icon,
    BrowserIcon,
    LayoutGridIcon,
    Note01Icon,
    PlusSignIcon,
  } from "../../lib/icons";
  import ToolButton from "./ToolButton.svelte";
  import type { WorkEnvironmentPanel } from "../../lib/work-environment";
  import * as m from "$shared/i18n/messages";
  let {
    spaceName,
    workTitle,
    profileLabel,
    panels = {},
    initialTabsOpen = false,
    onreturn,
    onpanelchange,
  }: {
    spaceName: string;
    workTitle: string;
    profileLabel: string;
    panels?: Partial<Record<WorkEnvironmentPanel, Snippet>>;
    initialTabsOpen?: boolean;
    onreturn: () => void;
    onpanelchange?: (panel: WorkEnvironmentPanel | null) => void;
  } = $props();
  const id = $props.id();
  let panel = $state<WorkEnvironmentPanel | null>(untrack(() => (initialTabsOpen ? "tabs" : null)));
  let escapePanel: WorkEnvironmentPanel | null = null;
  function restoreFocus(event: Event, key: WorkEnvironmentPanel) {
    if (escapePanel !== key) return;
    event.preventDefault();
    escapePanel = null;
    document.getElementById(`${id}-${key}`)?.focus();
  }
  function change(key: WorkEnvironmentPanel, open: boolean) {
    if (!open && panel !== key) return;
    panel = open ? key : null;
    onpanelchange?.(panel);
  }
  export function close() {
    if (panel) change(panel, false);
  }
  // A palette opens on hover intent as well as on click; the rest wait for a click.
  const tools = [
    { key: "notes" as const, label: m.work_env_notes(), icon: Note01Icon },
    { key: "create" as const, label: m.work_env_components(), icon: PlusSignIcon, palette: true },
    { key: "tabs" as const, label: m.work_env_tabs(), icon: BrowserIcon, prominent: true },
    { key: "media" as const, label: m.work_env_media(), icon: Attachment01Icon, palette: true },
    { key: "area" as const, label: m.work_env_area(), icon: LayoutGridIcon },
  ];
  let hover: ReturnType<typeof setTimeout> | undefined;
  function hoverOpen(key: WorkEnvironmentPanel) {
    clearTimeout(hover);
    hover = setTimeout(() => change(key, true), 180);
  }
  function hoverCancel() {
    clearTimeout(hover);
  }
</script>

{#snippet tool(
  key: WorkEnvironmentPanel,
  label: string,
  icon: (typeof tools)[number]["icon"],
  prominent = false,
  palette = false,
)}
  <Popover.Root open={panel === key} onOpenChange={(open) => change(key, open)}>
    <Popover.Trigger disabled={!panels[key]}>
      {#snippet child({ props })}
        <ToolButton
          {...props}
          id={`${id}-${key}`}
          {icon}
          {label}
          {prominent}
          active={panel === key}
          disabled={!panels[key]}
          onpointerenter={palette && panels[key] ? () => hoverOpen(key) : undefined}
          onpointerleave={palette ? hoverCancel : undefined}
        />
      {/snippet}
    </Popover.Trigger>
    <Popover.Content
      class="work-popover"
      sideOffset={10}
      collisionPadding={12}
      preventScroll={false}
      aria-label={label}
      onEscapeKeydown={() => (escapePanel = key)}
      onCloseAutoFocus={(event) => restoreFocus(event, key)}
    >
      {@render panels[key]?.()}
    </Popover.Content>
  </Popover.Root>
{/snippet}

<header class="chrome" data-zephium-work-chrome>
  <div class="identity">
    <button type="button" class="return" aria-label={m.work_env_return()} onclick={onreturn}>
      <Icon icon={ArrowLeft02Icon} size={16} />
    </button>
    <Popover.Root open={panel === "switcher"} onOpenChange={(open) => change("switcher", open)}>
      <Popover.Trigger class="work-identity" aria-label={m.work_env_switcher()}>
        <span class="space">{spaceName}</span>
        <span class="divider" aria-hidden="true">/</span>
        <strong>{workTitle}</strong>
        <span class="chevron"><Icon icon={ArrowDown01Icon} size={14} /></span>
      </Popover.Trigger>
      <Popover.Content
        class="work-popover work-popover-wide"
        align="start"
        sideOffset={10}
        collisionPadding={12}
        preventScroll={false}
        aria-label={m.work_env_switcher()}
      >
        {@render panels.switcher?.()}
      </Popover.Content>
    </Popover.Root>
  </div>
  <nav class="tools" aria-label={m.work_env_toolbar()}>
    {#each tools as entry (entry.key)}{@render tool(
        entry.key,
        entry.label,
        entry.icon,
        entry.prominent,
        entry.palette,
      )}{/each}
  </nav>
  <div class="profile">
    <Popover.Root open={panel === "profile"} onOpenChange={(open) => change("profile", open)}>
      <Popover.Trigger
        class="work-avatar"
        aria-label={m.work_env_profile({ name: profileLabel })}
        disabled={!panels.profile}
      >
        {profileLabel.slice(0, 1).toLocaleUpperCase()}
      </Popover.Trigger>
      <Popover.Content
        class="work-popover"
        align="end"
        sideOffset={10}
        collisionPadding={12}
        preventScroll={false}
        aria-label={m.work_env_profile({ name: profileLabel })}
      >
        {@render panels.profile?.()}
      </Popover.Content>
    </Popover.Root>
  </div>
</header>

<style>
  .chrome {
    position: absolute;
    inset-block-start: 0;
    inset-inline: 0;
    block-size: var(--work-header-height);
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    padding-inline: var(--work-header-inset-start) 12px;
    pointer-events: none;
    -webkit-app-region: drag;
  }

  .identity,
  .tools,
  .profile {
    pointer-events: auto;
    -webkit-app-region: no-drag;
  }

  .identity {
    display: flex;
    align-items: center;
    gap: 4px;
    min-inline-size: 0;
    justify-self: start;
  }

  .return {
    display: grid;
    place-items: center;
    inline-size: 28px;
    block-size: 28px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-muted);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      color var(--motion-fast) var(--ease-smooth);
  }

  .return:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  :global(.work-identity) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-inline-size: 100%;
    min-inline-size: 0;
    block-size: 30px;
    padding: 0 8px 0 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-smooth);
  }

  :global(.work-identity:hover),
  :global(.work-identity[data-state="open"]) {
    background: var(--color-fill-hover);
  }

  .space,
  .divider,
  .chevron {
    color: var(--color-muted);
  }

  .space,
  :global(.work-identity) strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  :global(.work-identity) strong {
    font-weight: 600;
  }

  .chevron {
    display: grid;
    place-items: center;
  }

  .tools {
    display: flex;
    align-items: center;
    gap: 2px;
    justify-self: center;
  }

  .profile {
    justify-self: end;
  }

  :global(.work-avatar) {
    display: grid;
    place-items: center;
    inline-size: 30px;
    block-size: 30px;
    border: 0;
    border-radius: 50%;
    background: var(--color-control);
    box-shadow: var(--shadow-control);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 600;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-smooth);
  }

  :global(.work-avatar:hover),
  :global(.work-avatar[data-state="open"]) {
    background: var(--color-control-hover);
  }

  :global(.work-popover) {
    z-index: 40;
    box-sizing: border-box;
    inline-size: min(380px, calc(100vw - 24px));
    max-block-size: calc(100vh - 96px);
    overflow: auto;
    padding: 8px;
    border-radius: var(--radius-menu);
    background: var(--color-menu);
    backdrop-filter: blur(12px) saturate(1.2);
    box-shadow: var(--shadow-popover);
    color: var(--color-text);
    font-size: var(--text-body);
    outline: none;
    transform-origin: var(--bits-floating-transform-origin, top);
    animation: work-popover-in var(--motion-slow) var(--ease-smooth);
  }

  :global(.work-popover-wide) {
    inline-size: min(440px, calc(100vw - 24px));
  }

  :global(.work-popover[data-state="closed"]) {
    animation: work-popover-out var(--motion-fast) var(--ease-out) forwards;
  }

  @keyframes work-popover-in {
    from {
      opacity: 0;
      transform: scale(0.96) translateY(-4px);
    }
  }

  @keyframes work-popover-out {
    to {
      opacity: 0;
      transform: scale(0.99);
    }
  }

  :global(.work-identity:focus-visible),
  :global(.work-avatar:focus-visible),
  .return:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  @media (forced-colors: active) {
    :global(.work-popover) {
      border: 1px solid ButtonText;
      backdrop-filter: none;
    }
  }
</style>
