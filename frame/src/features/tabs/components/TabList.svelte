<script lang="ts">
  import * as tabDrag from "$session/tab-drag.svelte";
  import { SvelteSet } from "svelte/reactivity";
  import type { SidebarSectionView, TabView } from "$shared/ipc/bindings";
  import { tabs } from "$domain/tabs";
  import type { Snippet } from "svelte";
  import type { TabTileProps } from "../lib/tab-tile";
  import FolderRow from "./FolderRow.svelte";
  import {
    sidebarDisplayUnits,
    collapsedSidebarUnits,
    type SidebarEntry,
  } from "../lib/sidebar-model";
  import SplitGroupRow from "./SplitGroupRow.svelte";
  import TabRow from "./TabRow.svelte";

  let {
    entries,
    section,
    variant = "list",
    essentialTile,
    label,
    splitting,
    onSelect,
  }: {
    entries: SidebarEntry[];
    section: SidebarSectionView;
    variant?: "list" | "essentials";
    essentialTile?: Snippet<[TabTileProps]>;
    label: string;
    splitting: boolean;
    onSelect: (id: string) => void;
  } = $props();

  let down: { x: number; y: number } | null = null;
  let dragId = "";
  let dragTitle = "";
  let dragging = $state(false);
  let ghost = $state<{ title: string; x: number; y: number } | null>(null);
  let pendingPointer = { x: 0, y: 0 };
  let frame = 0;
  let suppressClick = false;
  let displayUnits = $derived(sidebarDisplayUnits(section, entries, tabs.splitGroup()));
  const folded = new SvelteSet<string>();
  let hiddenUnits = $derived(collapsedSidebarUnits(displayUnits, folded, tabs.activeId()));

  function toggleFolder(key: string) {
    if (folded.has(key)) folded.delete(key);
    else folded.add(key);
  }

  function resetDrag() {
    if (frame !== 0) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
    down = null;
    dragId = "";
    dragTitle = "";
    dragging = false;
    ghost = null;
    document.body.style.cursor = "";
    tabDrag.end();
  }

  function handlePointerDown(event: PointerEvent, tab: TabView) {
    if (event.button !== 0) return;
    down = { x: event.clientX, y: event.clientY };
    dragId = tab.id;
    dragTitle = tab.title;
    dragging = false;
  }

  function handlePointerMove(event: PointerEvent) {
    if (down === null) return;

    if (!dragging && Math.abs(event.clientX - down.x) + Math.abs(event.clientY - down.y) > 4) {
      const target = event.currentTarget;
      if (target instanceof HTMLElement) target.setPointerCapture(event.pointerId);
      dragging = true;
      tabDrag.begin(dragId);
      document.body.style.cursor = "grabbing";
    }
    if (!dragging) return;

    pendingPointer = { x: event.clientX, y: event.clientY };
    if (frame !== 0) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      ghost = { title: dragTitle, ...pendingPointer };
      tabDrag.hover(pendingPointer.x, pendingPointer.y);
      tabs.dragOver(pendingPointer.x, pendingPointer.y);
    });
  }

  function handlePointerUp(event: PointerEvent) {
    const droppedElement = document.elementFromPoint(event.clientX, event.clientY);
    const essentialZone = droppedElement?.closest("[data-essentials-drop]");
    const listZone = droppedElement?.closest("[data-tabs-drop]");
    const before =
      droppedElement?.closest<HTMLElement>("[data-zephium-tab-id]")?.dataset.zephiumTabId ?? null;
    const didDrag = dragging;
    const id = dragId;
    const target = event.currentTarget;

    if (target instanceof HTMLElement && target.hasPointerCapture(event.pointerId)) {
      target.releasePointerCapture(event.pointerId);
    }
    resetDrag();

    if (!didDrag) return;
    suppressClick = true;
    if (essentialZone) {
      void tabDrag.move(id, true, before === id ? null : before);
      return;
    }
    if (listZone && section === "favorites") {
      void tabDrag.move(id, false, before === id ? null : before);
      return;
    }

    // Dropping onto another row in the sidebar pairs the two tabs, matching
    // the drop-onto-a-pane gesture over content. The shell splits the active
    // tab with a target, so the dragged tab is activated first; both are
    // ordered mutations through the same actor queue.
    const onto = rowUnderPointer(event.clientX, event.clientY, id);
    if (onto !== null) {
      tabs.activate(id);
      tabs.split(onto);
      return;
    }

    tabs.dropTab(id, event.clientX, event.clientY);
  }

  function rowUnderPointer(x: number, y: number, dragged: string): string | null {
    const element = document.elementFromPoint(x, y);
    const row = element?.closest("[data-zephium-tab-id]");
    if (!(row instanceof HTMLElement)) return null;
    const id = row.dataset.zephiumTabId;
    return id !== undefined && id !== dragged ? id : null;
  }

  function handlePointerCancel(event: PointerEvent) {
    const target = event.currentTarget;
    if (target instanceof HTMLElement && target.hasPointerCapture(event.pointerId)) {
      target.releasePointerCapture(event.pointerId);
    }
    resetDrag();
  }

  function handleContextMenu(event: MouseEvent, tab: TabView) {
    // A DOM menu cannot reach past the chrome WebView, so the tab menu is a
    // real native popup owned by Rust.
    event.preventDefault();
    resetDrag();
    tabs.openTabMenu(tab.id, event.clientX, event.clientY);
  }

  function select(id: string) {
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    onSelect(id);
  }
</script>

{#if entries.length > 0}
  <nav class={["tab-list", variant === "essentials" && "tab-list-flush"]} aria-label={label}>
    <ul
      class:flex={variant === "list"}
      class:dock-row={variant === "essentials"}
      style:--dock-columns={`repeat(${Math.min(Math.max(displayUnits.length, 1), 4)}, minmax(0, 1fr))`}
      class:flex-col={variant === "list"}
      class:tab-list-rows={variant === "list"}
      role="list"
    >
      {#each displayUnits as unit, index (unit.key)}
        {#if unit.kind === "folder"}
          <FolderRow
            expanded={!folded.has(unit.key) ||
              displayUnits.some(
                (child, index) =>
                  index > displayUnits.indexOf(unit) &&
                  child.depth > unit.depth &&
                  !hiddenUnits.has(child.key),
              )}
            ontoggle={() => toggleFolder(unit.key)}
            name={unit.node.kind.name}
            depth={unit.depth}
            class={[
              variant === "essentials" ? "col-span-full" : "",
              hiddenUnits.has(unit.key) ? "hidden" : "",
            ].join(" ")}
          />
        {:else if unit.kind === "split"}
          <SplitGroupRow
            tabs={unit.tabs.map((entry) => entry.tab)}
            activeId={tabs.activeId()}
            {splitting}
            closable={section === "today"}
            class={[
              variant === "essentials" ? "col-span-full" : "",
              hiddenUnits.has(unit.key) ? "hidden" : "",
            ].join(" ")}
            onSelect={select}
            onClose={tabs.close}
            onContextMenu={handleContextMenu}
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
            onPointerCancel={handlePointerCancel}
          />
        {:else if variant === "essentials" && unit.depth === 0 && essentialTile}
          {@render essentialTile({
            tab: unit.tab,
            active: unit.tab.id === tabs.activeId(),
            splitCandidate: splitting && unit.tab.id !== tabs.activeId(),
            onSelect: select,
            onContextMenu: handleContextMenu,
            onPointerDown: handlePointerDown,
            onPointerMove: handlePointerMove,
            onPointerUp: handlePointerUp,
            onPointerCancel: handlePointerCancel,
          })}
        {:else}
          <TabRow
            entranceIndex={index}
            tab={unit.tab}
            active={unit.tab.id === tabs.activeId()}
            depth={unit.depth}
            closable={section === "today"}
            class={[
              variant === "essentials" ? "col-span-full" : "",
              hiddenUnits.has(unit.key) ? "hidden" : "",
            ].join(" ")}
            splitCandidate={splitting && unit.tab.id !== tabs.activeId()}
            onSelect={select}
            onClose={tabs.close}
            onContextMenu={handleContextMenu}
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
            onPointerCancel={handlePointerCancel}
          />
        {/if}
      {/each}
    </ul>
  </nav>
{/if}

{#if ghost !== null}
  <div
    class="pointer-events-none fixed z-50 max-w-52 truncate rounded-row bg-raised px-3 py-2 text-[13px] text-text shadow-overlay"
    style:left={`${ghost.x + 12}px`}
    style:top={`${ghost.y + 8}px`}
    aria-hidden="true"
  >
    {ghost.title}
  </div>
{/if}

<style>
  .tab-list {
    padding-inline: var(--sidebar-inset);
    padding-block: 2px;
  }

  /* The dock owns the band's inset, so its site row adds none of its own. */
  .tab-list-flush {
    padding: 0;
  }

  /* The site row divides its width evenly, so one kept site fills the row and
     four share it. The track list arrives whole through the variable: a var()
     inside repeat() is not reliably substituted. */
  .dock-row {
    display: grid;
    grid-template-columns: var(--dock-columns, repeat(4, minmax(0, 1fr)));
    gap: var(--dock-gap);
  }

  /* One pixel between rows read as a stack of stripes rather than a list. */
  .tab-list-rows {
    gap: var(--sidebar-row-gap);
  }
</style>
