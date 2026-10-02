<script lang="ts">
  import { untrack } from "svelte";
  import {
    ArrowLeft01Icon,
    Bookmark02Icon,
    Delete02Icon,
    Folder01Icon,
    FolderAddIcon,
    Globe02Icon,
    PencilEdit02Icon,
  } from "@hugeicons/core-free-icons";
  import type { BookmarkView } from "$shared/ipc/bindings";
  import { commands } from "$shared/ipc/bindings";
  import { favicons } from "$domain/favicons";
  import { keymap } from "$domain/keymap";
  import { IS_MAC } from "$shared/platform";
  import { acceleratorKeys } from "$shared/lib/accelerator";
  import EmptyState from "$shared/ui/EmptyState";
  import FavIcon from "$shared/ui/FavIcon";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
  import * as m from "$shared/i18n/messages";
  import { BookmarksSession } from "../lib/bookmarks-session.svelte";
  import * as reveal from "../lib/reveal.svelte";

  let { profile, query }: { profile: string; query: string } = $props();

  let session = $state.raw(untrack(() => new BookmarksSession(profile)));
  let renaming = $state<string | null>(null);
  let draft = $state("");
  /** A folder whose removal waits for a second press, because it takes its
   *  contents with it. */
  let confirming = $state<string | null>(null);

  // Only the owning profile decides which session this is; the first view is
  // a requested bookmark when there is one, otherwise the top level.
  $effect(() => {
    const owner = profile;
    return untrack(() => {
      const current = new BookmarksSession(owner);
      session = current;
      const wanted = reveal.take();
      void (wanted ? current.reveal(wanted) : current.open(null));
      return () => current.stop();
    });
  });

  // Bookmark This Page while the panel is already open.
  $effect(() => {
    if (reveal.requested() === null) return;
    untrack(() => {
      const wanted = reveal.take();
      if (wanted) void session.reveal(wanted);
    });
  });

  // The frame owns the search field; mirror its value into the session.
  $effect(() => {
    const value = query;
    untrack(() => {
      if (value !== session.query) session.search(value);
    });
  });

  let addKeys = $derived.by(() => {
    const accelerator = keymap.all().find((entry) => entry.id === "bookmark.add")?.accelerator;
    if (!accelerator) return null;
    return acceleratorKeys(accelerator, IS_MAC).join(IS_MAC ? "" : "+");
  });

  function hostOf(url: string) {
    try {
      return new URL(url).host;
    } catch {
      return url;
    }
  }

  function activate(item: BookmarkView, event: MouseEvent | KeyboardEvent) {
    if (renaming === item.id) return;
    session.highlighted = null;
    if (item.url === null) {
      void session.open(item.id);
      return;
    }
    const newTab = event.metaKey || event.ctrlKey || ("button" in event && event.button === 1);
    void commands.browserOpenUrl(item.url, newTab);
  }

  function startRename(item: BookmarkView) {
    confirming = null;
    renaming = item.id;
    draft = item.title;
  }

  async function finishRename(item: BookmarkView, keep: boolean) {
    if (renaming !== item.id) return;
    renaming = null;
    const title = draft.trim();
    if (keep && title && title !== item.title) await session.rename(item.id, title);
  }

  async function remove(item: BookmarkView) {
    if (item.url === null && item.children > 0 && confirming !== item.id) {
      confirming = item.id;
      return;
    }
    confirming = null;
    await session.remove(item.id);
  }

  async function addFolder() {
    const id = await session.addFolder(m.bookmarks_new_folder_name());
    const created = session.items.find((item) => item.id === id);
    if (created) startRename(created);
  }

  const focusAndSelect = (node: HTMLInputElement) => {
    node.focus();
    node.select();
  };

  const scrollIntoView = (node: HTMLElement, lit: boolean) => {
    if (lit) node.scrollIntoView({ block: "nearest" });
    return {
      update(next: boolean) {
        if (next) node.scrollIntoView({ block: "nearest" });
      },
    };
  };
</script>

<div class="bookmarks">
  {#if !session.searching}
    <div class="place">
      {#if session.folder !== null}
        <IconButton
          icon={ArrowLeft01Icon}
          label={m.bookmarks_back()}
          size={14}
          buttonSize={24}
          onclick={() => void session.open(session.path.at(-2)?.id ?? null)}
        />
      {/if}
      <nav class="crumbs" aria-label={m.bookmarks_location()}>
        <button
          type="button"
          class="crumb"
          aria-current={session.folder === null ? "location" : undefined}
          onclick={() => void session.open(null)}>{m.tool_bookmarks()}</button
        >
        {#each session.path as crumb (crumb.id)}
          <span class="divider" aria-hidden="true">/</span>
          <button
            type="button"
            class="crumb"
            aria-current={crumb.id === session.folder ? "location" : undefined}
            onclick={() => void session.open(crumb.id)}>{crumb.title}</button
          >
        {/each}
      </nav>
      <IconButton
        icon={FolderAddIcon}
        label={m.bookmarks_new_folder()}
        size={15}
        buttonSize={24}
        onclick={() => void addFolder()}
      />
    </div>
  {/if}

  {#if session.error === "unavailable" || session.error === "capacity"}
    <EmptyState title={m.bookmarks_unavailable()} description={m.surface_retry()}>
      {#snippet icon()}<Icon icon={Bookmark02Icon} size={22} />{/snippet}
    </EmptyState>
  {:else if session.empty}
    <EmptyState
      title={session.searching
        ? m.bookmarks_no_matches()
        : session.folder === null
          ? m.bookmarks_empty()
          : m.bookmarks_folder_empty()}
      description={session.searching
        ? m.bookmarks_no_matches_help()
        : session.folder !== null
          ? m.bookmarks_folder_empty_help()
          : addKeys
            ? m.bookmarks_empty_help({ keys: addKeys })
            : m.bookmarks_empty_help_menu()}
    >
      {#snippet icon()}<Icon icon={Bookmark02Icon} size={22} />{/snippet}
    </EmptyState>
  {:else}
    {#if session.error === "full"}<p class="notice" role="status">{m.bookmarks_full()}</p>{/if}
    <ul class="list" aria-label={m.tool_bookmarks()}>
      {#each session.items as item (item.id)}
        {@const folder = item.url === null}
        {@const lit = session.highlighted === item.id}
        <li class="row" class:lit use:scrollIntoView={lit}>
          {#if renaming === item.id}
            <span class="glyph"
              >{#if folder}<Icon icon={Folder01Icon} size={16} />{:else}<FavIcon
                  image={favicons.image(item.icon)}
                  tone={favicons.tone(item.icon)}
                  size={16}
                  lit
                  fallback={Globe02Icon}
                />{/if}</span
            >
            <input
              class="rename"
              aria-label={m.bookmarks_rename()}
              maxlength={512}
              bind:value={draft}
              use:focusAndSelect
              onkeydown={(event) => {
                if (event.key === "Enter") void finishRename(item, true);
                if (event.key === "Escape") {
                  event.stopPropagation();
                  void finishRename(item, false);
                }
              }}
              onblur={() => void finishRename(item, true)}
            />
          {:else}
            <button
              type="button"
              class="open"
              title={item.url ?? undefined}
              onclick={(event) => activate(item, event)}
              onauxclick={(event) => {
                if (event.button === 1) activate(item, event);
              }}
            >
              <span class="glyph"
                >{#if folder}<Icon icon={Folder01Icon} size={16} />{:else}<FavIcon
                    image={favicons.image(item.icon)}
                    tone={favicons.tone(item.icon)}
                    size={16}
                    lit
                    fallback={Globe02Icon}
                  />{/if}</span
              >
              <span class="title">{item.title}</span>
              <span class="detail"
                >{folder
                  ? m.bookmarks_folder_count({ count: item.children })
                  : hostOf(item.url ?? "")}</span
              >
            </button>
            <span class="actions">
              <IconButton
                icon={PencilEdit02Icon}
                label={m.bookmarks_rename()}
                size={14}
                buttonSize={22}
                onclick={() => startRename(item)}
              />
              <IconButton
                icon={Delete02Icon}
                label={confirming === item.id
                  ? m.bookmarks_remove_folder_confirm({ count: item.children })
                  : m.bookmarks_remove()}
                class={confirming === item.id ? "confirm" : ""}
                size={14}
                buttonSize={22}
                onclick={() => void remove(item)}
              />
            </span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .bookmarks {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .place {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 8px 2px 10px;
  }

  .crumbs {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 2px;
    min-width: 0;
    overflow: hidden;
  }

  .crumb {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    padding: 3px 4px;
    border: 0;
    border-radius: var(--radius-inset);
    background: none;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    white-space: nowrap;
    text-overflow: ellipsis;
    cursor: default;
  }

  .crumb:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .crumb[aria-current="location"] {
    color: var(--color-text);
    font-weight: 550;
  }

  .divider {
    flex: none;
    color: var(--color-faint);
    font-size: var(--text-label);
  }

  .notice {
    margin: 4px 18px 0;
    font-size: var(--text-label);
    color: var(--color-warning);
  }

  .list {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 6px 8px 12px;
    overflow-y: auto;
    list-style: none;
  }

  .row {
    position: relative;
    display: flex;
    align-items: center;
    height: 34px;
    border-radius: var(--radius-row);
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  .row:hover,
  .row:focus-within {
    background: var(--row-active);
  }

  /* The bookmark just added holds the plate until the reader moves on. */
  .row.lit {
    background: var(--row-active);
    box-shadow: var(--row-rim);
  }

  .open {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) auto;
    flex: 1;
    align-items: center;
    gap: 10px;
    min-width: 0;
    height: 100%;
    padding: 0 10px;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .glyph {
    display: grid;
    place-items: center;
    color: var(--color-muted);
  }

  .row > .glyph {
    flex: none;
    width: 16px;
    margin-inline: 10px;
  }

  .title {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: var(--text-label);
    color: var(--color-text);
  }

  .detail {
    max-width: 110px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 11.5px;
    color: var(--color-faint);
  }

  /* Present but unseen until the row is in hand, so revealing them never
     moves what the pointer is on. */
  .actions {
    display: flex;
    gap: 2px;
    padding-inline-end: 6px;
    visibility: hidden;
  }

  .row:hover .actions,
  .row:focus-within .actions {
    visibility: visible;
  }

  .row:hover .detail,
  .row:focus-within .detail {
    visibility: hidden;
  }

  /* A folder's removal waits for a second press; the button says so in red. */
  .actions :global(.confirm) {
    color: var(--color-danger);
  }

  .rename {
    flex: 1;
    min-width: 0;
    height: 26px;
    margin-inline-end: 8px;
    padding: 0 8px;
    border: 1px solid var(--color-ring);
    border-radius: var(--radius-inset);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    outline: none;
    user-select: text;
  }

  @media (prefers-reduced-motion: reduce) {
    .row {
      transition: none;
    }
  }

  @media (forced-colors: active) {
    .row.lit {
      outline: 1px solid Highlight;
    }
  }
</style>
