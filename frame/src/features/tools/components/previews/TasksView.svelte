<script lang="ts">
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import ToolFrame from "../ToolFrame.svelte";
  import * as m from "$shared/i18n/messages";
  import Button from "$shared/ui/Button";
  import Field from "$shared/ui/Field";
  let viewProps: ToolHostProps = $props();
</script>

{#snippet editor()}<div class="shared-tool-editor">
    <Field
      label={m.field_name()}
      value={viewProps.state.title}
      maxlength={256}
      oninput={(event) => viewProps.edit({ title: event.currentTarget.value })}
    />
    <Button onclick={() => viewProps.edit({ composing: false })}>{m.action_cancel()}</Button>
  </div>{/snippet}
<ToolFrame
  {...viewProps}
  canCompose
  searchLabel={m.tool_search_tasks()}
  filters={[
    { value: "all", label: m.tool_all() },
    { value: "today", label: m.tool_today() },
    { value: "completed", label: m.tool_completed() },
  ]}
  children={viewProps.state.composing ? editor : undefined}
/>
