<script lang="ts">
  import { ArrowUpRight01Icon, Cancel01Icon, MoreHorizontalIcon } from "@hugeicons/core-free-icons";
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import { surface as browser } from "$domain/surface";
  import { loadHistoryPanel } from "$features/history";
  import Icon from "$shared/ui/Icon";
  import LazyView from "$shared/ui/LazyView";
  import Menu, { type MenuEntry } from "$shared/ui/Menu";
  import ToolFrame from "../ToolFrame.svelte";
  import * as m from "$shared/i18n/messages";
  let props: ToolHostProps = $props();

  const entries: MenuEntry[] = [
    { kind: "item", id: "expand", label: m.history_page_open(), icon: ArrowUpRight01Icon },
    { kind: "separator" },
    { kind: "item", id: "close", label: m.tool_close(), icon: Cancel01Icon },
  ];

  function act(id: string) {
    if (id === "expand") {
      void browser.open("history");
      props.onclose();
    } else if (id === "close") props.onclose();
  }
</script>

<ToolFrame
  {...props}
  caption={false}
  closable={false}
  searchLabel={m.history_search()}
  scrolls={false}
>
  {#snippet actions()}<Menu
      label={m.tool_more()}
      {entries}
      side="bottom"
      align="end"
      triggerClass="tool-overflow"
      onselect={act}
    >
      {#snippet trigger()}<Icon icon={MoreHorizontalIcon} size={16} />{/snippet}
    </Menu>{/snippet}
  <LazyView
    loader={loadHistoryPanel}
    loadingLabel={m.panel_loading()}
    failureLabel={m.panel_load_failed()}
    retryLabel={m.panel_retry()}
    >{#snippet children(Panel)}<Panel
        profile={props.profile}
        query={props.state.query}
      />{/snippet}</LazyView
  >
</ToolFrame>
