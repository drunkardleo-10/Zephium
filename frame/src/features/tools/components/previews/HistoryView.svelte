<script lang="ts">
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import ToolFrame from "../ToolFrame.svelte";
  import * as m from "$shared/i18n/messages";
  let viewProps: ToolHostProps = $props();
</script>

<ToolFrame {...viewProps} searchLabel={m.browser_search_history()} />
