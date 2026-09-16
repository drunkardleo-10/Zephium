<script lang="ts">
  import { Globe02Icon } from "@hugeicons/core-free-icons";
  import type { TabView } from "$shared/ipc/bindings";
  import FavIcon from "$shared/ui/FavIcon";
  import { favicons } from "$domain/favicons";

  let {
    tab,
    active,
    splitCandidate,
    class: className = "",
    onSelect,
    onContextMenu,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
  }: {
    tab: TabView;
    active: boolean;
    splitCandidate: boolean;
    class?: string;
    onSelect: (id: string) => void;
    onContextMenu: (event: MouseEvent, tab: TabView) => void;
    onPointerDown: (event: PointerEvent, tab: TabView) => void;
    onPointerMove: (event: PointerEvent) => void;
    onPointerUp: (event: PointerEvent) => void;
    onPointerCancel: (event: PointerEvent) => void;
  } = $props();
</script>

<li
  data-zephium-tab-id={tab.id}
  data-zephium-tab-url={tab.url ?? ""}
  data-zephium-projection-revision={tab.projection_revision}
  class={["min-w-0", className]}
>
  <button
    type="button"
    aria-current={active ? "page" : undefined}
    aria-label={tab.title || "Untitled essential"}
    title={tab.title || "Untitled essential"}
    class="flex aspect-square w-full cursor-default items-center justify-center rounded-lg bg-fill transition-[background-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] outline-none hover:bg-fill-hover"
    class:bg-fill-active={active}
    class:shadow-raised={active}
    class:ring-1={splitCandidate}
    class:ring-accent={splitCandidate}
    oncontextmenu={(event) => onContextMenu(event, tab)}
    onpointerdown={(event) => onPointerDown(event, tab)}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerCancel}
    onclick={() => onSelect(tab.id)}
  >
    <FavIcon
      image={favicons.image(tab.icon)}
      tone={favicons.tone(tab.icon)}
      loading={tab.loading}
      size={22}
      lit={active}
      fallback={Globe02Icon}
    />
    <span data-zephium-tab-label class="sr-only">{tab.title}</span>
  </button>
</li>
