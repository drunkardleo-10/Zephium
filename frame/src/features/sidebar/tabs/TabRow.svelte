<script lang="ts">
  import { BrowserIcon, Cancel01Icon, Globe02Icon } from "@hugeicons/core-free-icons";
  import type { TabView } from "../../../shared/ipc/bindings";
  import FavIcon from "../../../shared/ui/FavIcon.svelte";
  import Icon from "../../../shared/ui/Icon.svelte";

  let {
    tab,
    active,
    grouped = false,
    depth = 0,
    closable = true,
    class: className = "",
    splitCandidate,
    onSelect,
    onClose,
    onContextMenu,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
  }: {
    tab: TabView;
    active: boolean;
    grouped?: boolean;
    depth?: number;
    closable?: boolean;
    class?: string;
    splitCandidate: boolean;
    onSelect: (id: string) => void;
    onClose: (id: string) => void;
    onContextMenu: (event: MouseEvent, tab: TabView) => void;
    onPointerDown: (event: PointerEvent, tab: TabView) => void;
    onPointerMove: (event: PointerEvent) => void;
    onPointerUp: (event: PointerEvent) => void;
    onPointerCancel: (event: PointerEvent) => void;
  } = $props();

  function closeTab(event: MouseEvent) {
    event.stopPropagation();
    onClose(tab.id);
  }

  // A tab with no page yet is a different thing from a page whose site simply
  // supplies no icon, and the row should say which.
  let fallback = $derived(tab.url === null ? BrowserIcon : Globe02Icon);
</script>

<li
  data-zephium-tab-id={tab.id}
  data-zephium-tab-url={tab.url ?? ""}
  data-zephium-projection-revision={tab.projection_revision}
  class={[
    "group relative flex h-[34px] items-center text-[13.5px] text-text transition-[background-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)]",
    className,
  ]}
  class:rounded-md={!grouped}
  class:bg-fill-active={active}
  class:shadow-raised={active && !grouped}
  class:font-medium={active}
  class:hover:bg-fill-hover={!active}
  class:ring-1={splitCandidate}
  class:ring-accent={splitCandidate}
>
  <button
    type="button"
    class="flex min-w-0 flex-1 cursor-default items-center gap-2.5 self-stretch pe-1 text-start outline-none"
    class:rounded-s-md={!grouped}
    style:padding-inline-start={`${grouped ? 8 : 8 + Math.min(depth, 8) * 13}px`}
    aria-current={active ? "page" : undefined}
    aria-label={tab.title || "Untitled tab"}
    oncontextmenu={(event) => onContextMenu(event, tab)}
    onpointerdown={(event) => onPointerDown(event, tab)}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerCancel}
    onclick={() => onSelect(tab.id)}
  >
    <FavIcon favicon={tab.favicon} loading={tab.loading} lit={active} {fallback} />
    <span data-zephium-tab-label class="min-w-0 flex-1 truncate">{tab.title}</span>
  </button>
  {#if closable}
    <button
      type="button"
      aria-label={`Close ${tab.title || "tab"}`}
      title="Close tab"
      class="me-1.5 flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-sm text-faint opacity-0 transition-[background-color,color,opacity] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] outline-none group-hover:opacity-100 hover:bg-fill-pressed hover:text-text focus-visible:opacity-100"
      onclick={closeTab}
    >
      <Icon icon={Cancel01Icon} size={12} />
    </button>
  {/if}
</li>
