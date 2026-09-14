<script lang="ts">
  import { Popover } from "bits-ui";
  import { untrack, type Snippet } from "svelte";
  import type { WorkEnvironmentPanel } from "../lib/work-environment";
  import * as m from "$shared/i18n/messages";

  let {
    spaceName,
    workTitle,
    profileLabel,
    panels = {},
    children,
    composer,
    initialTabsOpen = false,
    taskLabel,
    onreturn,
    onpanelchange,
  }: {
    spaceName: string;
    workTitle: string;
    profileLabel: string;
    panels?: Partial<Record<WorkEnvironmentPanel, Snippet>>;
    children: Snippet;
    composer?: Snippet;
    initialTabsOpen?: boolean;
    /** Authoritative activity/count summary; no inferred progress percentage. */
    taskLabel?: string;
    onreturn: () => void;
    onpanelchange?: (panel: WorkEnvironmentPanel | null) => void;
  } = $props();
  const id = $props.id();
  let escapePanel: WorkEnvironmentPanel | null = null;
  function restoreFocus(event: Event, key: WorkEnvironmentPanel) {
    if (escapePanel !== key) return;
    event.preventDefault();
    escapePanel = null;
    document.getElementById(`${id}-${key}`)?.focus();
  }
  let panel = $state<WorkEnvironmentPanel | null>(untrack(() => (initialTabsOpen ? "tabs" : null)));
  const toolbar = $derived([
    { key: "notes" as const, label: m.work_env_notes() },
    { key: "create" as const, label: m.work_env_create() },
    { key: "tabs" as const, label: m.work_env_tabs() },
    { key: "media" as const, label: m.work_env_media() },
    { key: "connections" as const, label: m.work_env_connections() },
  ]);
  function change(key: WorkEnvironmentPanel, open: boolean) {
    if (!open && panel !== key) return;
    panel = open ? key : null;
    onpanelchange?.(panel);
  }
</script>

{#snippet control(key: WorkEnvironmentPanel, label: string, distinctive = false)}
  <Popover.Root open={panel === key} onOpenChange={(open) => change(key, open)}>
    <Popover.Trigger
      id={`${id}-${key}`}
      class={distinctive ? "work-env-chrome-button work-env-tabs-button" : "work-env-chrome-button"}
      disabled={!panels[key]}
      title={!panels[key] ? m.work_env_unavailable() : undefined}>{label}</Popover.Trigger
    >
    <Popover.Content
      class="work-env-work-popover"
      sideOffset={8}
      collisionPadding={16}
      preventScroll={false}
      aria-label={label}
      onEscapeKeydown={() => (escapePanel = key)}
      onCloseAutoFocus={(event) => restoreFocus(event, key)}
    >
      {@render panels[key]?.()}
    </Popover.Content>
  </Popover.Root>
{/snippet}

<section class="environment" aria-label={m.mode_work()}>
  <div class="canvas">{@render children()}</div>
  <header class="topbar">
    <div class="identity">
      <Popover.Root open={panel === "switcher"} onOpenChange={(open) => change("switcher", open)}>
        <Popover.Trigger
          class="work-env-chrome-button work-env-identity-button"
          aria-label={m.work_env_switcher()}
          ><span class="space">{spaceName}</span><strong>{workTitle}</strong><span
            aria-hidden="true">⌄</span
          ></Popover.Trigger
        >
        <Popover.Content
          class="work-env-work-popover"
          align="start"
          sideOffset={8}
          collisionPadding={16}
          preventScroll={false}
          aria-label={m.work_env_switcher()}
        >
          <button class="return-button" type="button" onclick={onreturn}
            >{m.work_env_return()}</button
          >
          {@render panels.switcher?.()}
        </Popover.Content>
      </Popover.Root>
    </div>
    <nav class="toolbar" aria-label={m.work_env_toolbar()}>
      {#each toolbar as tool (tool.key)}{@render control(
          tool.key,
          tool.label,
          tool.key === "tabs",
        )}{/each}
    </nav>
    <div class="profile">
      <Popover.Root open={panel === "profile"} onOpenChange={(open) => change("profile", open)}>
        <Popover.Trigger
          class="work-env-chrome-button work-env-avatar"
          aria-label={m.work_env_profile({ name: profileLabel })}
          disabled={!panels.profile}>{profileLabel.slice(0, 1).toLocaleUpperCase()}</Popover.Trigger
        >
        <Popover.Content
          class="work-env-work-popover"
          align="end"
          sideOffset={8}
          collisionPadding={16}
          preventScroll={false}
          aria-label={m.work_env_profile({ name: profileLabel })}
          >{@render panels.profile?.()}</Popover.Content
        >
      </Popover.Root>
    </div>
  </header>
  {#if panels.tasks}<aside class="activity">
      {@render control("tasks", taskLabel || m.work_env_tasks())}
    </aside>{/if}
  {#if composer}<div class="composer">{@render composer()}</div>{/if}
</section>

<style>
  .environment {
    position: relative;
    inline-size: 100%;
    block-size: 100%;
    min-inline-size: 0;
    min-block-size: 0;
    overflow: hidden;
    background: var(--color-canvas);
    color: var(--color-text);
    font-size: var(--text-body);
  }

  .canvas {
    position: absolute;
    inset: 0;
  }

  .topbar {
    position: absolute;
    inset-block-start: 16px;
    inset-inline: 16px;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: start;
    gap: 16px;
    pointer-events: none;
  }

  .identity,
  .toolbar,
  .profile {
    pointer-events: auto;
  }

  .identity {
    justify-self: start;
    min-inline-size: 0;
  }

  .profile {
    justify-self: end;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface);
    box-shadow: var(--shadow-float);
  }

  :global(.work-env-chrome-button) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-block-size: 32px;
    padding: 4px 12px;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    background: var(--color-surface);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-caption);
    cursor: pointer;
  }

  :global(.work-env-chrome-button:hover:not(:disabled)) {
    background: var(--color-fill-hover);
  }

  :global(.work-env-chrome-button:disabled) {
    color: var(--color-faint);
    cursor: default;
  }

  :global(.work-env-chrome-button[data-state="open"]) {
    background: var(--color-fill-active);
    border-color: var(--color-border-strong);
  }

  :global(.work-env-tabs-button) {
    background: var(--color-control);
    border-color: var(--color-border-strong);
    box-shadow: var(--shadow-control);
    font-weight: 600;
    padding-inline: 20px;
  }

  :global(.work-env-identity-button) {
    max-inline-size: 100%;
    justify-content: start;
  }

  .space {
    color: var(--color-muted);
  }

  .identity strong,
  .space {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .identity strong {
    font-weight: 500;
  }

  :global(.work-env-avatar) {
    inline-size: 32px;
    padding: 0;
    border-radius: 50%;
    border-color: var(--color-border-strong);
    font-weight: 600;
  }

  :global(.work-env-work-popover) {
    z-index: 20;
    inline-size: min(440px, calc(100vw - 32px));
    max-block-size: calc(100vh - 96px);
    box-sizing: border-box;
    overflow: auto;
    padding: 16px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-control);
    background: var(--color-surface);
    color: var(--color-text);
    box-shadow: var(--shadow-popover);
    font-size: var(--text-body);
  }

  .return-button {
    inline-size: 100%;
    text-align: start;
    margin-block-end: 8px;
    padding: 8px;
    background: var(--color-fill);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    font: inherit;
    cursor: pointer;
  }

  :global(.work-env-chrome-button:focus-visible),
  .return-button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .activity {
    position: absolute;
    inset-inline-end: 16px;
    inset-block-start: 50%;
  }

  .composer {
    position: absolute;
    inset-block-end: 16px;
    inset-inline-start: 50%;
    transform: translateX(-50%);
    inline-size: min(640px, calc(100% - 160px));
    max-block-size: 65%;
  }

  @media (width <= 860px) {
    .topbar {
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 8px;
    }

    .toolbar {
      grid-row: 2;
      grid-column: 1 / -1;
      justify-self: center;
    }

    .profile {
      grid-column: 2;
      grid-row: 1;
    }

    .composer {
      inline-size: calc(100% - 32px);
    }
  }
</style>
