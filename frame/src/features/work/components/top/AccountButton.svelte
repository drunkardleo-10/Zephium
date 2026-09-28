<script lang="ts">
  import Menu, { type MenuEntry } from "$shared/ui/Menu";
  import { commands } from "$shared/ipc/bindings";
  import Icon from "$shared/ui/Icon";
  import { UserIcon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";

  let { name }: { name: string } = $props();
  const entries = $derived<MenuEntry[]>([
    ...(name ? [{ kind: "heading" as const, label: name }] : []),
    { kind: "item", id: "settings.profiles", label: m.account_profile() },
    { kind: "item", id: "settings.account", label: m.account_manage() },
    { kind: "separator" },
    {
      kind: "item",
      id: "browser.settings",
      label: m.account_settings(),
      hint: "⌘,",
    },
  ]);
</script>

<Menu
  label={m.account_menu()}
  {entries}
  side="bottom"
  align="end"
  triggerClass="account-trigger"
  contentClass="works-menu-solid"
  onselect={(id) => void commands.runCommand(id)}
>
  {#snippet trigger()}<span class="avatar" aria-hidden="true"
      ><Icon icon={UserIcon} size={15} strokeWidth={1.7} /></span
    >{/snippet}
</Menu>

<style>
  :global(.works-menu-solid) {
    background: var(--color-float);
    backdrop-filter: none;
  }

  :global(.account-trigger) {
    display: grid;
    inline-size: 30px;
    block-size: 30px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-capsule);
    background: transparent;
    place-items: center;
    cursor: default;
    pointer-events: auto;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  :global(.account-trigger:hover),
  :global(.account-trigger[data-state="open"]) {
    background: var(--color-fill-hover);
  }

  :global(.account-trigger:focus-visible) {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  .avatar {
    display: grid;
    inline-size: 24px;
    block-size: 24px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-strong);
    color: var(--color-label-secondary);
    place-items: center;
    box-shadow: inset 0 0 0 0.5px var(--color-border);
  }
</style>
