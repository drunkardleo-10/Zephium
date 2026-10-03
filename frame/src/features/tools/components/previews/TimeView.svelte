<script lang="ts">
  import { ArrowUpRight01Icon } from "@hugeicons/core-free-icons";
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import { surface as browser } from "$domain/surface";
  import { tabs } from "$domain/tabs";
  import { loadTimePanel } from "$features/time";
  import IconButton from "$shared/ui/IconButton";
  import LazyView from "$shared/ui/LazyView";
  import * as m from "$shared/i18n/messages";
  import ToolFrame from "../ToolFrame.svelte";
  let props: ToolHostProps = $props();

  // The page is the same time in more depth, so the panel does not stay open
  // behind it.
  function expand() {
    void browser.open("time");
    props.onclose();
  }
</script>

<ToolFrame
  {...props}
  caption={false}
  filters={[
    { value: "today", label: m.tool_today() },
    { value: "week", label: m.tool_time_week() },
  ]}
>
  {#snippet actions()}<IconButton
      icon={ArrowUpRight01Icon}
      label={m.time_open_page()}
      onclick={expand}
    />{/snippet}
  <LazyView
    loader={loadTimePanel}
    loadingLabel={m.panel_loading()}
    failureLabel={m.panel_load_failed()}
    retryLabel={m.panel_retry()}
    >{#snippet children(Panel)}<Panel
        profile={props.profile}
        span={props.state.filter === "week" ? "week" : "day"}
        privateWindow={tabs.profile()?.kind === "incognito"}
        onopen={expand}
      />{/snippet}</LazyView
  >
</ToolFrame>
