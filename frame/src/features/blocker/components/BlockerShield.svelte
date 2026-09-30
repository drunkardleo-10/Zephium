<script lang="ts">
  import { Shield01Icon } from "@hugeicons/core-free-icons";
  import { blocker } from "$domain/blocker";
  import { shieldPresentation } from "$domain/blocker";
  import Icon from "$shared/ui/Icon";

  /** Name the standing beside the glyph, where there is room for it. */
  let { labelled = false, activated = 0 }: { labelled?: boolean; activated?: number } = $props();
  let Menu = $state<typeof import("./ProtectionMenu.svelte").default | null>(null);
  let failed = $state(false);
  let loading = false;
  $effect(() => {
    if (activated > 0 && Menu === null && !loading) {
      loading = true;
      void import("./ProtectionMenu.svelte")
        .then((module) => {
          Menu = module.default;
          failed = false;
        })
        .catch(() => {
          failed = true;
        })
        .finally(() => {
          loading = false;
        });
    }
  });
  let shield = $derived(shieldPresentation(blocker.status()));
</script>

<!--
  Load site controls on the first menu opening; keep startup to its standing.
  Coverage and source diagnostics stay in Settings.
-->
{#if Menu && labelled}
  <Menu {labelled} />
{:else if failed}
  <span role="status">Site controls unavailable. Reopen the menu to retry.</span>
{:else if shield.visible && labelled}
  <span
    class="flex items-center gap-2 px-2 py-1 text-[12.5px] text-muted"
    class:text-warning={shield.tone === "warning"}
    role="status"
  >
    <span class="flex h-4 w-4 items-center justify-center" class:opacity-40={shield.blocked}
      ><Icon icon={Shield01Icon} size={14} /></span
    >{shield.label}
  </span>
{:else if shield.visible}
  <span
    class="flex h-5 w-5 shrink-0 items-center justify-center"
    class:text-faint={shield.tone === "quiet"}
    class:opacity-40={shield.blocked}
    class:text-warning={shield.tone === "warning"}
    title={shield.label}
    role="img"
    aria-label={shield.label}
  >
    <Icon icon={Shield01Icon} size={14} />
  </span>
{/if}
