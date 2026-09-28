<script lang="ts">
  import { ArrowRight01Icon, PuzzleIcon } from "@hugeicons/core-free-icons";
  import { surface as browserPage } from "$domain/surface";
  import { webext } from "$domain/webext";
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";

  let { variant = "row" }: { variant?: "row" | "stack" } = $props();
</script>

{#if webext.isAvailable()}
  {#if variant === "stack"}
    <button
      type="button"
      role="menuitem"
      class="ui-menu-item shelf-item"
      title={m.webext_manage()}
      aria-label={m.webext_manage()}
      onclick={() => void browserPage.open("extensions")}
    >
      <span class="ui-menu-icon"><Icon icon={PuzzleIcon} size={16} /></span>
    </button>
  {:else}
    <button
      type="button"
      role="menuitem"
      class="ui-menu-item manage"
      onclick={() => void browserPage.open("extensions")}
    >
      <span>{m.webext_manage()}</span>
      <span class="chevron"><Icon icon={ArrowRight01Icon} size={14} /></span>
    </button>
  {/if}
{/if}

<style>
  .manage {
    width: 100%;
    border: 0;
    background: transparent;
    font: inherit;
    font-size: var(--text-body);
    text-align: start;
  }

  .manage:hover,
  .manage:focus-visible {
    outline: none;
    background: var(--row-active);
  }

  .chevron {
    display: grid;
    margin-inline-start: auto;
    color: var(--color-faint);
  }
</style>
