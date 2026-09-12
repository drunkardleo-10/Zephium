<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { Alert02Icon, LockIcon, Search01Icon } from "@hugeicons/core-free-icons";
  import { tabs } from "$domain/tabs";
  import { uiCommands as ui } from "$domain/ui-commands";
  import { commands } from "$shared/ipc/bindings";
  import Icon from "$shared/ui/Icon";
  import { addressSecurity, editingAddress, restingAddress } from "../lib/address-model";
  import type { Snippet } from "svelte";

  let { compact = false, shield }: { compact?: boolean; shield: Snippet } = $props();

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

<!--
  The input stays mounted in both shapes. Rust's synchronous presentation
  barrier commits and verifies the authoritative host through this exact
  element before revealing page content, so unmounting it at rail width would
  reject every presentation and leave page content permanently concealed. In
  compact mode it is present and correct but unpainted, and the launcher is the
  visible way to reach it.
-->
<form
  class="shrink-0"
  class:px-1.5={!compact}
  class:pb-2={!compact}
  onsubmit={submit}
  role="search"
>
  {#if compact}
    <div class="flex justify-center pb-1">
      <button
        type="button"
        class="chrome-button"
        aria-label={m.ui_search_or_enter_an_address()}
        title={m.ui_search_or_enter_an_address()}
        onclick={() => void commands.runCommand("launcher.toggle")}
      >
        <Icon icon={Search01Icon} size={16} />
      </button>
    </div>
  {/if}

  <div
    class:sr-only={compact}
    class:flex={!compact}
    class="focus-within:shadow-focus h-[34px] items-center gap-2 rounded-md bg-fill ps-2.5 pe-2 shadow-field transition-[background-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-out-quiet)] focus-within:bg-fill-hover hover:bg-fill-hover"
  >
    {#if !compact}
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
    {/if}

    <input
      bind:this={input}
      data-zephium-address
      type="text"
      aria-label={m.ui_address_and_search()}
      autocomplete="off"
      autocapitalize="off"
      enterkeyhint="go"
      placeholder={m.ui_search_or_enter_an_address()}
      spellcheck="false"
      {value}
      oninput={handleInput}
      onfocus={beginEditing}
      onblur={() => (editing = false)}
      class="min-w-0 flex-1 bg-transparent text-[13.5px] text-text outline-none placeholder:text-faint"
    />

    {#if !compact && activeUrl && security !== "none"}
      {@render shield()}
    {/if}
  </div>
</form>
