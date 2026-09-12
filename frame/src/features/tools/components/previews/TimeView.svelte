<script lang="ts">
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import ToolFrame from "../ToolFrame.svelte";
  import * as m from "$shared/i18n/messages";
  let viewProps: ToolHostProps = $props();
</script>

<ToolFrame
  {...viewProps}
  filters={[
    { value: "today", label: m.tool_today() },
    { value: "week", label: m.tool_time_week() },
  ]}
/>
