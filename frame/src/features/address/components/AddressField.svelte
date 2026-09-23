<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { Alert02Icon } from "@hugeicons/core-free-icons";
  import { settle } from "$domain/operations";
  import { tabs } from "$domain/tabs";
  import { uiCommands as ui } from "$domain/ui-commands";
  import { commands } from "$shared/ipc/bindings";
  import Icon from "$shared/ui/Icon";
  import type { Snippet } from "svelte";
  import { addressSecurity, editingAddress, restingAddress } from "../lib/address-model";

  let {
    compact = false,
    trailing,
  }: {
    compact?: boolean;
    /** Controls that act on the page, at the far end of the field. */
    trailing?: Snippet;
  } = $props();

  let input: HTMLInputElement;
  let editing = $state(false);
  let pending = $state(false);
  let failed = $state(false);
  let composing = false;
  let draft = $state("");
  let activeUrl = $derived(tabs.activeTab()?.url ?? "");
  let authoritativeValue = $derived(restingAddress(activeUrl));
  let value = $derived(editing ? draft : authoritativeValue);
  let security = $derived(addressSecurity(activeUrl));
  // Only a problem earns a glyph. A padlock on every encrypted page is the
  // default state of the web and says nothing; the field reads cleaner
  // without it, and the one case worth interrupting for still shows.
  let warning = $derived(!editing && security === "insecure");

  $effect(() => {
    const command = ui.uiCommand();
    if (command.seq === 0 || command.id !== "url.focus" || input === undefined) return;
    input.focus();
    input.select();
  });

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (composing || pending || !value.trim()) return;
    const id = tabs.activeId();
    if (id === null) return;
    const submitted = value;
    pending = true;
    failed = false;
    try {
      const result = await settle(commands.tabsNavigate(id, submitted));
      if (tabs.activeId() !== id) return;
      if (result.outcome === "failed" || result.outcome === "rejected") failed = true;
      else if (value === submitted) input.blur();
    } catch {
      if (tabs.activeId() === id) failed = true;
    } finally {
      pending = false;
    }
  }

  function handleInput(event: Event) {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) return;
    failed = false;
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
  class="shrink-0 !pl-0"
  class:pe-1.5={!compact}
  class:pb-2={!compact}
  onsubmit={submit}
  role="search"
>
  <!--
    A refused navigation rings the field, following the shared Field
    convention. The message is announced rather than drawn: a block of text
    appearing under the address bar would push the whole chrome down.
  -->
  <div
    class:sr-only={compact}
    class:flex={!compact}
    class:shadow-[inset_0_0_0_1px_var(--color-danger)]={failed}
    class="address-field focus-within:shadow-focus h-[34px] items-center gap-2 rounded-row bg-fill ps-2.5 pe-1 shadow-field transition-[background-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-out)] [--address-centering:20px] focus-within:bg-fill-hover hover:bg-fill-hover"
  >
    {#if !compact && warning}
      <span
        class="flex h-4 w-4 shrink-0 items-center justify-center text-danger"
        title={m.address_insecure()}
        role="img"
        aria-label={m.address_insecure()}
      >
        <Icon icon={Alert02Icon} size={14} />
      </span>
    {/if}

    <!--
      Resting, the text centres on the field rather than on the input. The
      trailing tray takes 26px and the field's own gutters differ by 6px, so
      the input's centre sits 10px left of the field's; padding twice that on
      the leading edge puts it back. Editing drops it and returns to the
      start, where a long URL has to begin.
    -->
    <input
      bind:this={input}
      data-zephium-address
      type="text"
      aria-label={m.ui_address_and_search()}
      autocomplete="off"
      autocapitalize="off"
      enterkeyhint="go"
      placeholder={m.ui_enter_an_address()}
      spellcheck="false"
      {value}
      oninput={handleInput}
      oncompositionstart={() => (composing = true)}
      oncompositionend={() => (composing = false)}
      onkeydown={(event) => {
        if (event.key === "Escape" && !event.isComposing) {
          event.preventDefault();
          failed = false;
          input.blur();
        }
      }}
      aria-invalid={failed || undefined}
      aria-describedby={failed ? "address-error" : undefined}
      onfocus={beginEditing}
      onblur={() => (editing = false)}
      style:text-align={editing ? "start" : "center"}
      style:padding-inline-start={editing ? "0" : "var(--address-centering)"}
      class="min-w-0 flex-1 bg-transparent text-[13.5px] text-label-secondary outline-none placeholder:text-faint focus:text-text"
    />

    {#if !compact && trailing}{@render trailing()}{/if}
  </div>
  {#if failed}<p id="address-error" role="alert" class="sr-only">{m.browser_nav_failed()}</p>{/if}
</form>
