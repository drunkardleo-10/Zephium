<script lang="ts">
  import { ContextMenu } from "bits-ui";
  import type { Snippet } from "svelte";
  import Icon from "$shared/ui/Icon";
  import { Archive01Icon, ArrowReloadHorizontalIcon, PencilEdit02Icon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";
  import "$shared/ui/Menu/popover.css";

  let {
    archived,
    disabled = false,
    children,
    onrename,
    onarchive,
    onrestore,
  }: {
    archived: boolean;
    disabled?: boolean;
    children: Snippet;
    /** Absent where there is no room to type a name, as in the rail. */
    onrename?: () => void;
    onarchive: () => void;
    onrestore: () => void;
  } = $props();
</script>

<ContextMenu.Root>
  <ContextMenu.Trigger {disabled}>
    {#snippet child({ props })}<div {...props} class="project-trigger">
        {@render children()}
      </div>{/snippet}
  </ContextMenu.Trigger>
  <ContextMenu.Portal>
    <ContextMenu.Content class="ui-menu" collisionPadding={10}>
      {#if onrename}<ContextMenu.Item class="ui-menu-item" onSelect={onrename}>
          <span class="ui-menu-icon" aria-hidden="true"
            ><Icon icon={PencilEdit02Icon} size={15} /></span
          >{m.work_project_rename()}
        </ContextMenu.Item>{/if}
      {#if archived}
        <ContextMenu.Item class="ui-menu-item" onSelect={onrestore}>
          <span class="ui-menu-icon" aria-hidden="true"
            ><Icon icon={ArrowReloadHorizontalIcon} size={15} /></span
          >{m.work_project_restore()}
        </ContextMenu.Item>
      {:else}
        <ContextMenu.Item class="ui-menu-item" onSelect={onarchive}>
          <span class="ui-menu-icon" aria-hidden="true"
            ><Icon icon={Archive01Icon} size={15} /></span
          >{m.work_project_archive()}
        </ContextMenu.Item>
      {/if}
    </ContextMenu.Content>
  </ContextMenu.Portal>
</ContextMenu.Root>

<style>
  .project-trigger {
    display: contents;
  }
</style>
