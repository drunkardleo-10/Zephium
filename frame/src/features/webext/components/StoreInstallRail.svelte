<script lang="ts">
  import { Download01Icon } from "@hugeicons/core-free-icons";
  import { extensions } from "$domain/extensions";
  import { tabs } from "$domain/tabs";
  import { webext } from "$domain/webext";
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";

  let listing = $derived(
    webext.isAvailable() && extensions.isChromeStoreListing(tabs.activeTab()?.url ?? ""),
  );
  let failure = $derived(webext.review() === null ? webext.error() : null);
</script>

<!--
  At rail width the address field, and the Add button under it, are hidden;
  on a Web Store listing the rail carries the button instead.
-->
{#if listing}
  <button
    type="button"
    class="install"
    title={failure ?? m.webext_store_install()}
    aria-label={m.webext_store_install()}
    aria-busy={webext.isPreparing() || undefined}
    disabled={webext.isPreparing()}
    data-extension-store-install
    onclick={() => {
      const id = tabs.activeId();
      if (id !== null) void webext.prepare(id);
    }}
  >
    <Icon icon={Download01Icon} size={16} />
  </button>
{/if}

<style>
  .install {
    display: grid;
    flex: none;
    place-items: center;
    align-self: center;
    width: 40px;
    height: var(--row-sidebar);
    margin-block: 2px 6px;
    border: 0;
    border-radius: var(--radius-row);
    background: var(--color-lit);
    color: var(--color-on-lit);
    cursor: default;
    animation: interface-fade var(--motion-fast) var(--ease-out) both;
    transition: scale var(--motion-slow) var(--ease-spring);
  }

  .install:active {
    scale: 0.94;
    transition-duration: var(--motion-instant);
  }

  .install:disabled {
    opacity: 0.6;
  }

  .install:hover:not(:disabled) {
    background: var(--color-lit-hover);
  }
</style>
