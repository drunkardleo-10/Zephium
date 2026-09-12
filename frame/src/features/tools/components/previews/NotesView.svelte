<script lang="ts">
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import ToolFrame from "../ToolFrame.svelte";
  import * as m from "$shared/i18n/messages";
  import Button from "$shared/ui/Button";
  import Field from "$shared/ui/Field";
  import TextArea from "$shared/ui/TextArea";
  let viewProps: ToolHostProps = $props();
</script>

{#snippet editor()}<div class="shared-tool-editor">
    <Field
      label={m.field_name()}
      value={viewProps.state.title}
      maxlength={256}
      oninput={(event) => viewProps.edit({ title: event.currentTarget.value })}
    />
    <TextArea
      label={m.tool_notes()}
      rows={8}
      value={viewProps.state.draft}
      maxlength={16384}
      oninput={(event) => viewProps.edit({ draft: event.currentTarget.value })}
    />
    <Button onclick={() => viewProps.edit({ composing: false })}>{m.action_cancel()}</Button>
  </div>{/snippet}
<ToolFrame
  {...viewProps}
  canCompose
  searchLabel={m.tool_search_notes()}
  children={viewProps.state.composing ? editor : undefined}
/>
