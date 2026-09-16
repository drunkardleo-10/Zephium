<script lang="ts">
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import LazyView from "$shared/ui/LazyView";
  import { loadHistoryPanel } from "$features/history";
  import ToolFrame from "../ToolFrame.svelte";
  import * as m from "$shared/i18n/messages";
  let props: ToolHostProps = $props();
</script>

<ToolFrame {...props} searchLabel={m.history_search()} scrolls={false}>
  <LazyView
    loader={loadHistoryPanel}
    loadingLabel={m.panel_loading()}
    failureLabel={m.panel_load_failed()}
    retryLabel={m.panel_retry()}
    >{#snippet children(Panel)}<Panel
        profile={props.profile}
        host={props.host}
        query={props.state.query}
      />{/snippet}</LazyView
  >
</ToolFrame>
