<script lang="ts">
  import { rememberScroll } from "$shared/ui/scroll-memory";
  import type { TabView } from "$shared/ipc/bindings";
  import { tabs } from "$domain/tabs";
  import FavIcon from "$shared/ui/FavIcon";
  import { favicons } from "$domain/favicons";
  import { Add01Icon, Globe02Icon } from "@hugeicons/core-free-icons";
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";

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
  The rail reads top-down like the expanded list it stands in for, and ends
  the same way, with the row that opens a new tab. `title` gives a real OS
  tooltip, which is the only label that can appear outside the chrome
  WebView's rectangle.
-->
<div
  use:rememberScroll={`${tabs.profile()?.id}/${tabs.activeSpaceId()}/rail`}
  class="flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-contain py-2"
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
          class="flex h-10 w-10 cursor-default items-center justify-center rounded-full transition-[background-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] outline-none"
          class:bg-fill-active={active}
          class:shadow-raised={active}
          class:hover:bg-fill-hover={!active}
          oncontextmenu={(event) => handleContextMenu(event, tab)}
          onclick={() => onSelect(tab.id)}
        >
          <FavIcon
            image={favicons.image(tab.icon)}
            tone={favicons.tone(tab.icon)}
            loading={tab.loading}
            lit={active}
            size={20}
            fallback={Globe02Icon}
          />
          <span data-zephium-tab-label class="sr-only">{tab.title}</span>
        </button>
      </li>
    {/each}
    <li>
      <button
        type="button"
        title={m.new_tab()}
        aria-label={m.new_tab()}
        class="flex h-10 w-10 cursor-default items-center justify-center rounded-full text-faint transition-[background-color,color] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] outline-none hover:bg-fill-hover hover:text-label-secondary"
        onclick={tabs.open}
      >
        <Icon icon={Add01Icon} size={17} />
      </button>
    </li>
  </ul>
</div>
