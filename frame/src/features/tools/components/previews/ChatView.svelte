<script lang="ts">
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import ToolFrame from "../ToolFrame.svelte";
  import Icon from "$shared/ui/Icon";
  import { ArrowUp02Icon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  let viewProps: ToolHostProps = $props();
</script>

<ToolFrame {...viewProps}
  >{#snippet footer()}<form
      class="shared-tool-composer"
      onsubmit={(event) => {
        event.preventDefault();
        viewProps.edit({ submitted: true });
      }}
    >
      <textarea
        aria-label={m.tool_ai_placeholder()}
        placeholder={m.tool_ai_placeholder()}
        rows="3"
        maxlength={16384}
        value={viewProps.state.draft}
        oninput={(event) => viewProps.edit({ draft: event.currentTarget.value })}></textarea><button
        type="submit"
        disabled={!viewProps.state.draft.trim()}
        aria-label={m.action_preview()}><Icon icon={ArrowUp02Icon} size={16} /></button
      >
    </form>
    {#if viewProps.state.submitted}<p class="shared-tool-caption" role="status">
        {m.tool_prompt_note()}
      </p>{/if}{/snippet}</ToolFrame
>
