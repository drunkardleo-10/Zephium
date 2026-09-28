<script lang="ts">
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";
  import type { CatalogEntry } from "../lib/catalog";
  import { catalogIcon } from "../lib/catalog-icons";

  let { entry, installed, onget }: { entry: CatalogEntry; installed: boolean; onget: () => void } =
    $props();

  let icon = $state<string | null>(null);

  $effect(() => {
    let current = true;
    void catalogIcon(entry.icon).then((url) => {
      if (current) icon = url;
    });
    return () => (current = false);
  });
</script>

<li class="card">
  <span class="badge" aria-hidden="true">
    {#if icon}<img src={icon} alt="" />{:else}{entry.name.charAt(0)}{/if}
  </span>
  <span class="text">
    <span class="name">{entry.name}</span>
    <span class="blurb">{entry.blurb()}</span>
  </span>
  {#if installed}
    <span class="installed">{m.webext_already_installed()}</span>
  {:else}
    <Button
      size="compact"
      variant="secondary"
      aria-label={m.webext_get_named({ name: entry.name })}
      onclick={onget}>{m.webext_get()}</Button
    >
  {/if}
</li>

<style>
  .card {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 10px 10px 10px 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-panel);
    background: var(--color-raised);
  }

  .badge {
    display: grid;
    flex: none;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
    font-size: var(--text-body);
    font-weight: 550;
    color: var(--color-text);
  }

  /* Store icons carry their own shape; the tile only holds their place. */
  .badge:has(img) {
    background: none;
  }

  .badge img {
    width: 32px;
    height: 32px;
  }

  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .name,
  .blurb {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .name {
    font-size: var(--text-body);
    color: var(--color-text);
  }

  .blurb {
    font-size: var(--text-label);
    color: var(--color-muted);
  }

  .installed {
    flex: none;
    font-size: var(--text-label);
    color: var(--color-faint);
  }
</style>
