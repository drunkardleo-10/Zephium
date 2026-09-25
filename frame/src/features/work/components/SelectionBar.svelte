<script lang="ts">
  import { tick } from "svelte";
  import Icon from "$shared/ui/Icon";
  import DashboardSquare01Icon from "@hugeicons/core-free-icons/DashboardSquare01Icon";
  import AlignLeftIcon from "@hugeicons/core-free-icons/AlignLeftIcon";
  import { ArrowDown01Icon, LayoutGridIcon, MinusSignIcon } from "../lib/icons";
  import type { Alignment, Arrangement } from "../lib/selection";
  import * as m from "$shared/i18n/messages";
  let {
    count,
    owned,
    onarrange,
    onalign,
    onarea,
    onask,
    onremove,
  }: {
    count: number;
    /** Selected elements the person placed; only those can be grouped or removed. */
    owned: number;
    onarrange: (how: Arrangement) => void;
    onalign: (how: Alignment) => void;
    onarea: () => void;
    onask: () => void;
    onremove: () => void;
  } = $props();
  type Choice = { id: string; label: string };
  const menus = $derived({
    arrange: {
      label: m.work_select_arrange(),
      icon: DashboardSquare01Icon,
      choices: [
        { id: "grid", label: m.work_select_grid() },
        { id: "row", label: m.work_select_row() },
        { id: "stack", label: m.work_select_stack() },
        { id: "tidy", label: m.work_select_tidy() },
      ] as Choice[],
      choose: (id: string) => onarrange(id as Arrangement),
    },
    align: {
      label: m.work_select_align(),
      icon: AlignLeftIcon,
      choices: [
        { id: "left", label: m.work_select_align_left() },
        { id: "top", label: m.work_select_align_top() },
        { id: "center", label: m.work_select_align_center() },
      ] as Choice[],
      choose: (id: string) => onalign(id as Alignment),
    },
  });
  let open = $state<"arrange" | "align" | null>(null);
  let root = $state<HTMLElement>();
  async function toggle(menu: "arrange" | "align") {
    open = open === menu ? null : menu;
    await tick();
    root?.querySelector<HTMLElement>(".popover [role='menuitem']")?.focus();
  }
  function close(refocus = false) {
    const was = open;
    open = null;
    if (refocus && was) root?.querySelector<HTMLElement>(`[data-menu="${was}"]`)?.focus();
  }
  function keys(event: KeyboardEvent) {
    if (!open) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close(true);
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const items = [...(root?.querySelectorAll<HTMLElement>(".popover [role='menuitem']") ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    const step = event.key === "ArrowDown" ? 1 : -1;
    items[(at + step + items.length) % items.length]?.focus();
  }
</script>

<svelte:window
  onpointerdowncapture={(event) => {
    if (open && !root?.contains(event.target as Node)) close();
  }}
/>

<div
  bind:this={root}
  class="selection-bar nodrag nopan nowheel"
  role="toolbar"
  tabindex="-1"
  aria-label={m.work_select_toolbar({ count })}
  onkeydown={keys}
>
  {#each ["arrange", "align"] as const as menu (menu)}
    <div class="menu">
      <button
        type="button"
        data-menu={menu}
        aria-haspopup="menu"
        aria-expanded={open === menu}
        class:open={open === menu}
        onclick={() => void toggle(menu)}
        ><Icon icon={menus[menu].icon} size={14} />{menus[menu].label}<Icon
          icon={ArrowDown01Icon}
          size={12}
        /></button
      >
      {#if open === menu}<div class="popover" role="menu" aria-label={menus[menu].label}>
          {#each menus[menu].choices as choice (choice.id)}<button
              type="button"
              role="menuitem"
              onclick={() => {
                close();
                menus[menu].choose(choice.id);
              }}>{choice.label}</button
            >{/each}
        </div>{/if}
    </div>
  {/each}
  <span class="separator"></span>
  <button type="button" disabled={!owned} onclick={onarea}>
    <Icon icon={LayoutGridIcon} size={14} />{m.work_select_area()}
  </button>
  <button type="button" onclick={onask}>{m.work_select_ask()}</button>
  <span class="separator"></span>
  <button type="button" class="danger" disabled={!owned} onclick={onremove}>
    <Icon icon={MinusSignIcon} size={14} />{m.work_select_remove()}
  </button>
</div>

<style>
  /* A floating bar of controls on main's control recipe: fill, and only fill. */
  .selection-bar {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    border-radius: var(--radius-control);
    background: var(--color-float);
    box-shadow: var(--shadow-popover);
    outline: none;
  }

  .selection-bar button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    block-size: 28px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    box-shadow: none;
    color: var(--color-on-control);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    white-space: nowrap;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .popover button {
    justify-content: flex-start;
    inline-size: 100%;
  }

  .selection-bar button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .selection-bar button:disabled {
    color: var(--color-faint);
  }

  .selection-bar button:hover:not(:disabled) {
    background: var(--color-control);
    box-shadow: var(--shadow-control);
  }

  .selection-bar button.open,
  .selection-bar button:active:not(:disabled) {
    background: var(--color-control-pressed);
  }

  .selection-bar button.danger:hover:not(:disabled) {
    color: var(--color-danger);
  }

  .menu {
    position: relative;
  }

  .popover {
    position: absolute;
    inset-block-start: calc(100% + 8px);
    inset-inline-start: -4px;
    z-index: 1;
    display: grid;
    min-inline-size: 140px;
    padding: 4px;
    border-radius: var(--radius-control);
    background: var(--color-float);
    box-shadow: var(--shadow-popover);
    transform-origin: top left;
    animation: popover-in var(--motion-fast) var(--ease-out);
  }

  .separator {
    inline-size: 1px;
    block-size: 16px;
    margin-inline: 4px;
    background: var(--color-border);
  }

  @keyframes popover-in {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
  }
</style>
