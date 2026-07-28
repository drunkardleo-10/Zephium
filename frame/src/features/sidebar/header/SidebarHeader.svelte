<script lang="ts">
  import { ArrowLeft02Icon, ArrowRight02Icon, Refresh01Icon } from "@hugeicons/core-free-icons";
  import * as tabs from "../../../domain/tabs/tabs.svelte";
  import IconButton from "../../../shared/ui/IconButton.svelte";
  import WindowControls from "./WindowControls.svelte";

  let { isMac }: { isMac: boolean } = $props();

  let active = $derived(tabs.activeTab());
</script>

<header
  class="flex h-9 shrink-0 items-center gap-px pe-1.5"
  class:ps-[52px]={isMac}
  class:ps-1.5={!isMac}
  aria-label="Navigation"
>
  <IconButton
    icon={ArrowLeft02Icon}
    label="Back"
    disabled={active?.can_go_back !== true}
    onclick={tabs.backActive}
  />
  <IconButton
    icon={ArrowRight02Icon}
    label="Forward"
    disabled={active?.can_go_forward !== true}
    onclick={tabs.forwardActive}
  />
  <IconButton
    icon={Refresh01Icon}
    label="Reload"
    disabled={active === undefined}
    onclick={tabs.reloadActive}
  />

  <span class="flex-1" aria-hidden="true"></span>

  {#if !isMac}
    <WindowControls />
  {/if}
</header>
