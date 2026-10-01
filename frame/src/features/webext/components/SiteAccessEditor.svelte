<script lang="ts">
  import { Cancel01Icon } from "@hugeicons/core-free-icons";
  import { onMount } from "svelte";
  import type { WebExtensionView } from "$shared/ipc/bindings";
  import { webext } from "$domain/webext";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";

  let { extension, onclose }: { extension: WebExtensionView; onclose: () => void } = $props();
  // svelte-ignore state_referenced_locally
  let sites = $state(extension.access === "sites" ? [...extension.sites] : []);
  let draft = $state("");
  let field = $state<HTMLInputElement>();

  onMount(() => field?.focus());

  // The browser keeps only the host of whatever is typed; this just avoids
  // listing the same entry twice while editing.
  function add() {
    const site = draft.trim().toLowerCase();
    if (site && !sites.includes(site)) sites = [...sites, site];
    draft = "";
  }

  async function done() {
    if (draft.trim()) add();
    await webext.setAccess(extension.id, "sites", sites);
    onclose();
  }
</script>

<div class="editor" role="group" aria-label={m.webext_sites_title({ name: extension.name })}>
  <p class="title">{m.webext_sites_title({ name: extension.name })}</p>
  {#if sites.length === 0}
    <p class="note">{m.webext_sites_none()}</p>
  {:else}
    <ul class="sites">
      {#each sites as site (site)}
        <li>
          <span>{site}</span>
          <button
            type="button"
            aria-label={m.webext_sites_remove({ site })}
            onclick={() => (sites = sites.filter((entry) => entry !== site))}
            ><Icon icon={Cancel01Icon} size={12} /></button
          >
        </li>
      {/each}
    </ul>
  {/if}
  <form
    class="add"
    onsubmit={(event) => {
      event.preventDefault();
      add();
    }}
  >
    <input
      bind:this={field}
      bind:value={draft}
      type="text"
      inputmode="url"
      autocomplete="off"
      spellcheck="false"
      placeholder={m.webext_sites_placeholder()}
      aria-label={m.webext_sites_add()}
      onkeydown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          onclose();
        }
      }}
    />
    <Button size="compact" variant="secondary" type="submit">{m.webext_sites_add()}</Button>
    <Button size="compact" variant="primary" onclick={() => void done()}>{m.webext_done()}</Button>
  </form>
</div>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0 12px 12px 52px;
    padding: 10px 12px;
    border-radius: var(--radius-row);
    background: var(--color-fill);
  }

  .title {
    margin: 0;
    font-size: var(--text-label);
    color: var(--color-muted);
  }

  .note {
    margin: 0;
    font-size: var(--text-label);
    color: var(--color-faint);
  }

  .sites {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .sites li {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 4px 3px 9px;
    border-radius: 999px;
    background: var(--color-raised);
    font-size: var(--text-label);
    color: var(--color-text);
  }

  .sites button {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background: none;
    color: var(--color-muted);
    cursor: pointer;
  }

  .sites button:hover {
    background: var(--row-active);
    color: var(--color-text);
  }

  .add {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .add input {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding-inline: 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control-compact);
    background: var(--color-page);
    font: inherit;
    font-size: var(--text-label);
    color: var(--color-text);
  }

  .add input:focus-visible {
    border-color: var(--color-accent);
    outline: none;
  }
</style>
