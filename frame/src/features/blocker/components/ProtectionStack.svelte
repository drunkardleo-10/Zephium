<script lang="ts">
  import { CursorPointer02Icon, Shield01Icon } from "@hugeicons/core-free-icons";
  import { blocker, blockerSites, hiding } from "$domain/blocker";
  import { commands } from "$shared/ipc/bindings";
  import { expandBriefly } from "$session/sidebar-mode.svelte";
  import Icon from "$shared/ui/Icon";

  let status = $derived(blocker.status());
  let site = $derived(status.site);
  let on = $derived(status.applied_enabled === true);
  let ready = $derived(on && site !== null && site.ready && !site.busy);
  let label = $derived(
    !on
      ? "Turn on ad and tracker protection"
      : site?.paused
        ? "Protection paused on this site. Resume"
        : "Protection on for this site. Pause",
  );

  async function toggle() {
    if (!on) {
      await blocker.setEnabled(true);
      return;
    }
    const context = site?.context;
    if (!context) return;
    const result = await blockerSites.changeSite(context, {
      kind: "pause",
      paused: !site?.paused,
    });
    if (result.state === "processed") void commands.tabsReload(context.tab);
  }

  // The rail has no room for the hiding bar, so the column opens for the
  // session and folds back when it ends.
  async function hide() {
    const context = site?.context;
    if (!context) return;
    const restore = expandBriefly();
    if (!(await hiding.start(context, restore))) restore();
  }
</script>

{#if status.protection !== "unavailable"}
  <button
    type="button"
    role="menuitem"
    class="ui-menu-item shelf-item"
    class:dim={!on || site?.paused}
    title={label}
    aria-label={label}
    disabled={on && !ready}
    onclick={() => void toggle()}
  >
    <span class="ui-menu-icon"><Icon icon={Shield01Icon} size={16} /></span>
  </button>
  {#if ready}
    <button
      type="button"
      role="menuitem"
      class="ui-menu-item shelf-item"
      title="Hide elements"
      aria-label="Hide elements"
      onclick={() => void hide()}
    >
      <span class="ui-menu-icon"><Icon icon={CursorPointer02Icon} size={16} /></span>
    </button>
  {/if}
{/if}

<style>
  .shelf-item:not(.dim) :global(.ui-menu-icon) {
    color: var(--color-success);
  }

  .shelf-item:disabled {
    opacity: 0.45;
  }
</style>
