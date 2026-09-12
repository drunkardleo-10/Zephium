<script lang="ts">
  import type { ToolHostProps } from "$session/tool-drafts.svelte";
  import { loadTasks } from "$features/tasks";
  import LazyView from "$shared/ui/LazyView";
  import * as m from "$shared/i18n/messages";
  let props: ToolHostProps = $props();
</script>

<LazyView
  loader={loadTasks}
  loadingLabel={m.surface_loading()}
  failureLabel={m.surface_render_failed()}
  retryLabel={m.surface_retry()}
  >{#snippet children(View)}<View
      profile={props.profile}
      host={props.host}
      onclose={props.onclose}
      onback={props.onback}
      ondrag={props.ondrag}
    />{/snippet}</LazyView
>
