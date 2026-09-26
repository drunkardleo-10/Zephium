<script lang="ts">
  import { DropdownMenu } from "bits-ui";
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
  let open = $state(false);
  // The menu opens where the pointer asked for it, as a native context menu does.
  let point = { x: 0, y: 0 };
  const anchor = {
    getBoundingClientRect: () => new DOMRect(point.x, point.y, 0, 0),
  };
</script>

<div
  class="project-trigger"
  role="presentation"
  oncontextmenu={(event) => {
    if (disabled) return;
    event.preventDefault();
    point = { x: event.clientX, y: event.clientY };
    open = true;
  }}
>
  {@render children()}
</div>
<DropdownMenu.Root bind:open>
  <DropdownMenu.Portal>
    <DropdownMenu.Content
      class="ui-menu"
      customAnchor={anchor}
      side="bottom"
      align="start"
      sideOffset={2}
      collisionPadding={10}
      onCloseAutoFocus={(event) => event.preventDefault()}
    >
      {#if onrename}<DropdownMenu.Item class="ui-menu-item" onSelect={onrename}>
          <span class="ui-menu-icon" aria-hidden="true"
            ><Icon icon={PencilEdit02Icon} size={15} /></span
          >{m.work_project_rename()}
        </DropdownMenu.Item>{/if}
      {#if archived}
        <DropdownMenu.Item class="ui-menu-item" onSelect={onrestore}>
          <span class="ui-menu-icon" aria-hidden="true"
            ><Icon icon={ArrowReloadHorizontalIcon} size={15} /></span
          >{m.work_project_restore()}
        </DropdownMenu.Item>
      {:else}
        <DropdownMenu.Item class="ui-menu-item" onSelect={onarchive}>
          <span class="ui-menu-icon" aria-hidden="true"
            ><Icon icon={Archive01Icon} size={15} /></span
          >{m.work_project_archive()}
        </DropdownMenu.Item>
      {/if}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>

<style>
  .project-trigger {
    display: contents;
  }
</style>
