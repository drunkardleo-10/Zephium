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
    /** The row; it gets the menu's opener for its own options button. */
    children: Snippet<[(x: number, y: number) => void]>;
    onrename: () => void;
    onarchive: () => void;
    onrestore: () => void;
  } = $props();
  let open = $state(false);
  let point = { x: 0, y: 0 };
  const anchor = { getBoundingClientRect: () => new DOMRect(point.x, point.y, 0, 0) };
  function openAt(x: number, y: number) {
    if (disabled) return;
    point = { x, y };
    open = true;
  }
</script>

<div
  class="row-trigger"
  role="presentation"
  oncontextmenu={(event) => {
    event.preventDefault();
    openAt(event.clientX, event.clientY);
  }}
>
  {@render children(openAt)}
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
      <DropdownMenu.Item class="ui-menu-item" onSelect={onrename}>
        <span class="ui-menu-icon" aria-hidden="true"
          ><Icon icon={PencilEdit02Icon} size={15} /></span
        >{m.work_rename()}
      </DropdownMenu.Item>
      {#if archived}
        <DropdownMenu.Item class="ui-menu-item" onSelect={onrestore}>
          <span class="ui-menu-icon" aria-hidden="true"
            ><Icon icon={ArrowReloadHorizontalIcon} size={15} /></span
          >{m.work_restore()}
        </DropdownMenu.Item>
      {:else}
        <DropdownMenu.Item class="ui-menu-item" onSelect={onarchive}>
          <span class="ui-menu-icon" aria-hidden="true"
            ><Icon icon={Archive01Icon} size={15} /></span
          >{m.work_archive()}
        </DropdownMenu.Item>
      {/if}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>

<style>
  .row-trigger {
    display: contents;
  }
</style>
