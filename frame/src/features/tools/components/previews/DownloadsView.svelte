<script lang="ts">
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import { loadDownloadsList } from "$features/downloads";
  import ToolFrame from "../ToolFrame.svelte";
  import LazyView from "$shared/ui/LazyView";
  import * as m from "$shared/i18n/messages";
  let viewProps: ToolHostProps = $props();
</script>

<ToolFrame {...viewProps} caption={false}
  ><LazyView
    loader={loadDownloadsList}
    loadingLabel={m.download_loading()}
    failureLabel={m.surface_render_failed()}
    retryLabel={m.surface_retry()}
    >{#snippet children(DownloadsList)}<DownloadsList
        profile={viewProps.profile}
      />{/snippet}</LazyView
  ></ToolFrame
>
