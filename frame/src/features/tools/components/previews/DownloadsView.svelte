<script lang="ts">
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import { DownloadsList } from "$features/downloads";
  import ToolFrame from "../ToolFrame.svelte";
  let viewProps: ToolHostProps = $props();
</script>

<ToolFrame {...viewProps} preview={false}><DownloadsList profile={viewProps.profile} /></ToolFrame>
