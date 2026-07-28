<script lang="ts">
  import { Alert02Icon, LockIcon, Search01Icon } from "@hugeicons/core-free-icons";
  import * as tabs from "../../../domain/tabs/tabs.svelte";
  import * as ui from "../../../domain/ui-commands/ui-commands.svelte";
  import Icon from "../../../shared/ui/Icon.svelte";
  import { addressSecurity, editingAddress, restingAddress } from "./address-model";
  import BlockerStatus from "../shield/BlockerShield.svelte";

  let input: HTMLInputElement;
  let editing = $state(false);
  let draft = $state("");
  let activeUrl = $derived(tabs.activeTab()?.url ?? "");
  let authoritativeValue = $derived(restingAddress(activeUrl));
  let value = $derived(editing ? draft : authoritativeValue);
  let security = $derived(addressSecurity(activeUrl));
  let leading = $derived(
    editing || security === "none"
      ? { icon: Search01Icon, label: "Search or enter an address", danger: false }
      : security === "secure"
        ? { icon: LockIcon, label: "Connection is encrypted", danger: false }
        : { icon: Alert02Icon, label: "Connection is not encrypted", danger: true },
  );

  $effect(() => {
    const command = ui.uiCommand();
    if (command.seq === 0 || command.id !== "url.focus" || input === undefined) return;
    input.focus();
    input.select();
  });

  function submit(event: SubmitEvent) {
    event.preventDefault();
    const id = tabs.activeId();
    if (id !== null) tabs.navigate(id, value);
    input.blur();
  }

  function handleInput(event: Event) {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) return;
    draft = target.value;
  }

  function beginEditing() {
    // Keep the resting chrome quiet while preserving the complete location
    // when the user explicitly enters edit mode.
    draft = editingAddress(activeUrl) || value;
    editing = true;
    input.select();
  }
</script>

<form class="shrink-0 px-1.5 pb-2" onsubmit={submit} role="search">
  <div
    class="flex h-[34px] items-center gap-2 rounded-md bg-fill ps-2.5 pe-2 shadow-field transition-[background-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] focus-within:bg-fill-hover focus-within:shadow-focus hover:bg-fill-hover"
  >
    <span
      class="flex h-4 w-4 shrink-0 items-center justify-center"
      class:text-faint={!leading.danger}
      class:text-danger={leading.danger}
      title={leading.label}
      role="img"
      aria-label={leading.label}
    >
      <Icon icon={leading.icon} size={14} />
    </span>

    <input
      bind:this={input}
      data-zephium-address
      type="text"
      aria-label="Address and search"
      autocomplete="off"
      autocapitalize="off"
      enterkeyhint="go"
      placeholder="Search or enter an address"
      spellcheck="false"
      {value}
      oninput={handleInput}
      onfocus={beginEditing}
      onblur={() => (editing = false)}
      class="min-w-0 flex-1 bg-transparent text-[13.5px] text-text outline-none placeholder:text-faint"
    />

    <BlockerStatus />
  </div>
</form>
