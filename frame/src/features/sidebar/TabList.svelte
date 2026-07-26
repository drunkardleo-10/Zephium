<script lang="ts">
  import type { SidebarSectionView, TabView } from "../../ipc/bindings";
  import * as tabs from "../../state/tabs.svelte";
  import EssentialTile from "./EssentialTile.svelte";
  import FolderRow from "./FolderRow.svelte";
  import { sidebarDisplayUnits, type SidebarEntry } from "./sidebar-model";
  import SplitGroupRow from "./SplitGroupRow.svelte";
  import TabRow from "./TabRow.svelte";

  let {
    entries,
    section,
    variant = "list",
    label,
    splitting,
    onSelect,
  }: {
    entries: SidebarEntry[];
    section: SidebarSectionView;
    variant?: "list" | "essentials";
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
      document.body.style.cursor = "grabbing";
    }
    if (!dragging) return;

    pendingPointer = { x: event.clientX, y: event.clientY };
    if (frame !== 0) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      ghost = { title: dragTitle, ...pendingPointer };
      tabs.dragOver(pendingPointer.x, pendingPointer.y);
    });
  }

  function handlePointerUp(event: PointerEvent) {
    const didDrag = dragging;
    const id = dragId;
    const target = event.currentTarget;

    if (target instanceof HTMLElement && target.hasPointerCapture(event.pointerId)) {
      target.releasePointerCapture(event.pointerId);
    }
    resetDrag();

    if (!didDrag) return;
    suppressClick = true;

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
  <nav class="px-1.5 py-0.5" aria-label={label}>
    <ul
      class:grid={variant === "essentials"}
      class:grid-cols-[repeat(auto-fill,minmax(46px,1fr))]={variant === "essentials"}
      class:gap-1.5={variant === "essentials"}
      class:flex={variant === "list"}
      class:flex-col={variant === "list"}
      class:gap-px={variant === "list"}
      role="list"
    >
      {#each displayUnits as unit (unit.key)}
        {#if unit.kind === "folder"}
          <FolderRow
            name={unit.node.kind.name}
            depth={unit.depth}
            class={variant === "essentials" ? "col-span-full" : ""}
          />
        {:else if unit.kind === "split"}
          <SplitGroupRow
            tabs={unit.tabs.map((entry) => entry.tab)}
            activeId={tabs.activeId()}
            {splitting}
            closable={section === "today"}
            class={variant === "essentials" ? "col-span-full" : ""}
            onSelect={select}
            onClose={tabs.close}
            onContextMenu={handleContextMenu}
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
            onPointerCancel={handlePointerCancel}
          />
        {:else if variant === "essentials" && unit.depth === 0}
          <EssentialTile
            tab={unit.tab}
            active={unit.tab.id === tabs.activeId()}
            splitCandidate={splitting && unit.tab.id !== tabs.activeId()}
            onSelect={select}
            onContextMenu={handleContextMenu}
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
            onPointerCancel={handlePointerCancel}
          />
        {:else}
          <TabRow
            tab={unit.tab}
            active={unit.tab.id === tabs.activeId()}
            depth={unit.depth}
            closable={section === "today"}
            class={variant === "essentials" ? "col-span-full" : ""}
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
    class="pointer-events-none fixed z-50 max-w-52 truncate rounded-md bg-raised px-3 py-2 text-[13px] text-text shadow-overlay"
    style:left={`${ghost.x + 12}px`}
    style:top={`${ghost.y + 8}px`}
    aria-hidden="true"
  >
    {ghost.title}
  </div>
{/if}
