<script lang="ts">
  import { rememberScroll } from "$shared/ui/scroll-memory";
  import type { TabView } from "$shared/ipc/bindings";
  import { tabs } from "$domain/tabs";
  import FavIcon from "$shared/ui/FavIcon";
  import { favicons } from "$domain/favicons";
  import { Globe02Icon } from "@hugeicons/core-free-icons";

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

<!--
  The rail is vertically centred in the column. `title` gives a real OS
  tooltip, which is the only label that can appear outside the chrome
  WebView's rectangle.
-->
<div
  use:rememberScroll={`${tabs.profile()?.id}/${tabs.activeSpaceId()}/rail`}
  class="flex min-h-0 flex-1 flex-col justify-center overflow-y-auto overscroll-contain py-2"
>
  <ul class="flex flex-col items-center gap-1" role="list" aria-label="Open tabs">
    {#each entries as tab (tab.id)}
      {@const active = tab.id === tabs.activeId()}
      <li
        data-zephium-tab-id={tab.id}
        data-zephium-tab-url={tab.url ?? ""}
        data-zephium-projection-revision={tab.projection_revision}
        class="relative"
      >
        <button
          type="button"
          title={tab.title || "Untitled tab"}
          aria-current={active ? "page" : undefined}
          aria-label={tab.title || "Untitled tab"}
          class="flex h-10 w-10 cursor-default items-center justify-center rounded-lg transition-[background-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] outline-none"
          class:bg-fill-active={active}
          class:shadow-raised={active}
          class:hover:bg-fill-hover={!active}
          oncontextmenu={(event) => handleContextMenu(event, tab)}
          onclick={() => onSelect(tab.id)}
        >
          <FavIcon
            image={favicons.image(tab.icon)}
            loading={tab.loading}
            lit={active}
            size={20}
            fallback={Globe02Icon}
          />
          <span data-zephium-tab-label class="sr-only">{tab.title}</span>
        </button>
      </li>
    {/each}
  </ul>
</div>
