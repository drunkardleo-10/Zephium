<script lang="ts">
  import { ArrowLeft02Icon, PuzzleIcon } from "@hugeicons/core-free-icons";
  import { onMount } from "svelte";
  import { surface as browser } from "$domain/surface";
  import { webext } from "$domain/webext";
  import { commands } from "$shared/ipc/bindings";
  import EmptyState from "$shared/ui/EmptyState";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
  import Menu from "$shared/ui/Menu";
  import * as m from "$shared/i18n/messages";
  import { catalog, storeListing } from "../lib/catalog";
  import CatalogCard from "./CatalogCard.svelte";
  import ExtensionRow from "./ExtensionRow.svelte";

  let extensions = $derived(webext.list());
  let failure = $derived(webext.error());
  let installedIds = $derived(new Set(extensions.map((extension) => extension.id)));

  onMount(() => void webext.refresh());

  const openInTab = (url: string) => void commands.browserOpenUrl(url, true);
</script>

<section class="library-shell">
  <header class="internal-toolbar">
    <IconButton
      icon={ArrowLeft02Icon}
      label={m.settings_back()}
      onclick={() => void browser.open(null)}
    /><span class="toolbar-current">{m.webext_page_title()}</span>
    <span class="spacer"></span>
    <Menu
      label={m.webext_install_file()}
      triggerClass="toolbar-action"
      align="end"
      entries={[
        { kind: "item", id: "package", label: m.webext_install_package() },
        { kind: "item", id: "folder", label: m.webext_install_folder() },
      ]}
      onselect={(id) => void webext.chooseFile(id === "folder")}
    >
      {#snippet trigger()}{m.webext_install_file()}{/snippet}
    </Menu>
    <button
      type="button"
      class="toolbar-action"
      onclick={() => openInTab("https://chromewebstore.google.com/")}
      >{m.webext_open_store()}</button
    >
  </header>

  <div class="content">
    {#if failure !== null}
      <p class="failure" role="alert">{failure}</p>
    {/if}

    {#if extensions.length === 0}
      <div class="empty">
        <EmptyState title={m.webext_empty_title()} description={m.webext_empty_help()}>
          {#snippet icon()}<Icon icon={PuzzleIcon} size={28} />{/snippet}
        </EmptyState>
      </div>
    {:else}
      <h2 class="heading">
        {m.webext_installed()} <span class="count">{extensions.length}</span>
      </h2>
      <ul class="group">
        {#each extensions as extension (extension.id)}
          <ExtensionRow {extension} />
        {/each}
      </ul>
    {/if}

    <h2 class="heading">{m.webext_recommended()}</h2>
    {#each catalog as group (group.title)}
      <h3 class="group-title">{group.title()}</h3>
      <ul class="catalog">
        {#each group.entries as entry (entry.id)}
          <CatalogCard
            {entry}
            installed={installedIds.has(entry.id)}
            onget={() => openInTab(storeListing(entry.id))}
          />
        {/each}
      </ul>
    {/each}

    <p class="note">{m.webext_blocker_note()}</p>
  </div>
</section>

<style>
  .spacer {
    flex: 1;
  }

  .internal-toolbar :global(.toolbar-action) {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding-inline: 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: none;
    font: inherit;
    color: var(--color-muted);
    cursor: pointer;
    transition:
      background-color var(--motion-instant) var(--ease-smooth),
      color var(--motion-instant) var(--ease-smooth);
  }

  .internal-toolbar :global(.toolbar-action:hover),
  .internal-toolbar :global(.toolbar-action[data-state="open"]) {
    background: var(--row-active);
    color: var(--color-text);
  }

  /* The same reading column as History, so the destinations line up. */
  .content {
    --library-gutter: clamp(16px, 4vw, 48px);
    --library-column: 720px;

    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-inline: max(var(--library-gutter), calc((100% - var(--library-column)) / 2));
    padding-block: 8px 32px;
  }

  .failure {
    margin: 0 0 12px;
    font-size: var(--text-label);
    color: var(--color-danger);
  }

  .heading {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    margin: 18px 0 8px;
    padding-inline: 10px;
    font-size: var(--text-label);
    font-weight: 550;
    letter-spacing: 0.01em;
    color: var(--color-muted);
  }

  .heading:first-child,
  .failure + .heading {
    margin-block-start: 4px;
  }

  .count {
    font-weight: 400;
    color: var(--color-faint);
  }

  .group {
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-panel);
    background: var(--color-raised);
  }

  .catalog {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .group-title {
    margin: 14px 0 6px;
    padding-inline: 10px;
    font-size: var(--text-label);
    font-weight: 450;
    color: var(--color-faint);
  }

  .heading + .group-title {
    margin-block-start: 0;
  }

  .note {
    margin: 18px 0 0;
    padding-inline: 10px;
    font-size: var(--text-label);
    line-height: 1.45;
    color: var(--color-faint);
  }

  .empty {
    padding-block: 12px;
  }
</style>
