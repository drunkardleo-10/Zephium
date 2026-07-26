<script lang="ts">
  import { Add01Icon, IncognitoIcon, UserCircleIcon } from "@hugeicons/core-free-icons";
  import { commands } from "../../ipc/bindings";
  import * as tabs from "../../state/tabs.svelte";
  import Icon from "../../ui/Icon.svelte";
  import IconButton from "../../ui/IconButton.svelte";

  let profile = $derived(tabs.profile());
  let name = $derived(profile?.name ?? "Personal");
  let incognito = $derived(profile?.kind === "incognito");

  function anchorOf(event: MouseEvent): DOMRect | null {
    const target = event.currentTarget;
    if (!(target instanceof HTMLButtonElement)) return null;
    return target.getBoundingClientRect();
  }

  function openProfileMenu(event: MouseEvent) {
    const anchor = anchorOf(event);
    if (anchor !== null) void commands.profileMenuPopup(anchor.left, anchor.top);
  }

  function openAddMenu(event: MouseEvent) {
    const anchor = anchorOf(event);
    if (anchor !== null) void commands.addMenuPopup(anchor.left, anchor.top, tabs.canSplitActive());
  }
</script>

<!--
  The profile is a security boundary, so it reads as identity rather than as
  another switcher chip. On Windows and Linux this menu is also the app menu.
-->
<footer class="shrink-0 px-1.5 pt-1 pb-2">
  <div class="flex h-8 items-center gap-1">
    <button
      type="button"
      aria-label={`Profile: ${name}`}
      aria-haspopup="menu"
      title={name}
      class="flex h-[34px] min-w-0 flex-1 items-center gap-2 rounded-md px-2 text-start text-[13.5px] text-muted transition-[background-color,color] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] outline-none hover:bg-fill-hover hover:text-text"
      onclick={openProfileMenu}
    >
      <span
        class="flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-full bg-fill"
        class:text-accent={incognito}
      >
        <Icon icon={incognito ? IncognitoIcon : UserCircleIcon} size={15} />
      </span>
      <span class="min-w-0 flex-1 truncate">{name}</span>
    </button>

    <IconButton icon={Add01Icon} label="New tab" size={16} haspopup onclick={openAddMenu} />
  </div>
</footer>
