<script lang="ts">
  import { Globe02Icon } from "@hugeicons/core-free-icons";
  import { tabs } from "$domain/tabs";
  import type { TabView } from "$shared/ipc/bindings";
  import FavIcon from "$shared/ui/FavIcon";

  let {
    entries,
    onSelect,
  }: {
    entries: TabView[];
    onSelect: (id: string) => void;
  } = $props();

  function handleContextMenu(event: MouseEvent, tab: TabView) {
    event.preventDefault();
    tabs.openTabMenu(tab.id, event.clientX, event.clientY);
  }
</script>

{#if entries.length > 0}
  <ul class="flex shrink-0 flex-col items-center gap-1 pb-1" role="list" aria-label="Essentials">
    {#each entries as tab (tab.id)}
      {@const active = tab.id === tabs.activeId()}
      <li
        data-zephium-tab-id={tab.id}
        data-zephium-tab-url={tab.url ?? ""}
        data-zephium-projection-revision={tab.projection_revision}
      >
        <button
          type="button"
          title={tab.title || "Untitled essential"}
          aria-current={active ? "page" : undefined}
          aria-label={tab.title || "Untitled essential"}
          class="flex h-10 w-10 cursor-default items-center justify-center rounded-lg bg-fill transition-[background-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] outline-none hover:bg-fill-hover"
          class:bg-fill-active={active}
          class:shadow-raised={active}
          oncontextmenu={(event) => handleContextMenu(event, tab)}
          onclick={() => onSelect(tab.id)}
        >
          <FavIcon
            favicon={tab.favicon}
            loading={tab.loading}
            size={20}
            lit={active}
            fallback={Globe02Icon}
          />
          <span data-zephium-tab-label class="sr-only">{tab.title}</span>
        </button>
      </li>
    {/each}
  </ul>
{/if}
