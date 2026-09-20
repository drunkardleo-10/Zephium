<script lang="ts">
  import { Globe02Icon } from "@hugeicons/core-free-icons";
  import { tabs } from "$domain/tabs";
  import type { TabView } from "$shared/ipc/bindings";
  import FavIcon from "$shared/ui/FavIcon";
  import { favicons } from "$domain/favicons";

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
          class="essential-rail-tile flex h-10 w-10 cursor-default items-center justify-center rounded-full transition-[background-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] outline-none"
          class:essential-rail-current={active}
          oncontextmenu={(event) => handleContextMenu(event, tab)}
          onclick={() => onSelect(tab.id)}
        >
          <FavIcon
            image={favicons.image(tab.icon)}
            tone={favicons.tone(tab.icon)}
            loading={tab.loading}
            size={18}
            lit={active}
            fallback={Globe02Icon}
          />
          <span data-zephium-tab-label class="sr-only">{tab.title}</span>
        </button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  /* The same plate the dock's tiles wear, at the rail's size: a hairline ring
     around a quiet ground, so a row of unrelated brand marks stays calm. */
  .essential-rail-tile {
    background: var(--color-card);
    box-shadow: inset 0 0 0 1px var(--color-border);
  }

  .essential-rail-tile:hover {
    background: var(--row-hover);
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  .essential-rail-current {
    background: var(--row-active);
    box-shadow: var(--shadow-raised);
  }
</style>
