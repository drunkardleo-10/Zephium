<script lang="ts" generics="T extends {id: string}">
  import type { Snippet } from "svelte";
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";
  let {
    title,
    rows,
    row,
    editor,
    editing,
    query,
    onquery,
    loading,
    error,
    oncreate,
    onclose,
    onback,
    ondrag,
    onexit,
    hasMore,
    onmore,
    trash,
    ontrash,
    status,
    filters,
  }: {
    status?: Snippet;
    filters?: Snippet;
    title: string;
    rows: readonly T[];
    row: Snippet<[T]>;
    editor: Snippet;
    editing: boolean;
    query: string;
    onquery: (value: string) => void;
    loading: boolean;
    error: string | null;
    oncreate: () => void;
    onclose?: () => void;
    onback: () => void;
    ondrag?: () => void;
    onexit?: () => void;
    hasMore: boolean;
    onmore: () => void;
    trash: boolean;
    ontrash: () => void;
  } = $props();
</script>

<section class="resource-panel" aria-label={title}>
  <header
    role="group"
    aria-label={title}
    onpointerdown={(event) => {
      if (event.button === 0 && !(event.target as HTMLElement).closest("button,input")) ondrag?.();
    }}
  >
    <div class="heading">
      {#if onexit}<Button
          variant="ghost"
          size="compact"
          onclick={onexit}
          aria-label={m.panel_back()}>‹</Button
        >{/if}
      <h1>{title}</h1>
    </div>
    <div>
      <Button
        variant="ghost"
        size="compact"
        onclick={oncreate}
        disabled={loading || trash || !!error}>{m.resource_new()}</Button
      >{#if onclose}<Button variant="ghost" size="compact" onclick={onclose}
          >{m.resource_close()}</Button
        >{/if}
    </div>
  </header>
  {#if status}{@render status()}{/if}
  <div class="resource-layout" class:editing>
    <div class="resource-list">
      <label class="search"
        ><input
          aria-label={m.resource_search()}
          placeholder={m.resource_search()}
          type="search"
          value={query}
          oninput={(event) => onquery(event.currentTarget.value)}
          maxlength="512"
        /></label
      ><Button variant="ghost" size="compact" aria-pressed={trash} onclick={ontrash}
        >{trash ? m.resource_show_active() : m.resource_show_trash()}</Button
      >
      {#if filters}{@render filters()}{/if}
      {#if rows.length >= 1000}<p>{m.resource_refine_search()}</p>{/if}
      {#if error}<p role="alert">{m.resource_load_failed()}</p>
        <Button size="compact" onclick={onmore}>{m.surface_retry()}</Button>{/if}
      {#if loading && rows.length === 0}<p role="status">{m.surface_loading()}</p>{/if}
      <ul>
        {#each rows as item (item.id)}<li>{@render row(item)}</li>{:else}{#if !loading && !error}<li
              class="empty"
            >
              {trash ? m.resource_trash_empty() : m.resource_empty()}
            </li>{/if}{/each}
      </ul>
      {#if hasMore}<Button onclick={onmore} disabled={loading}>{m.resource_more()}</Button>{/if}
    </div>
    {#if editing}<div class="resource-editor">
        <div class="back"><Button size="compact" onclick={onback}>{m.resource_back()}</Button></div>
        {@render editor()}
      </div>{:else}<div class="resource-welcome"><p>{m.resource_select()}</p></div>{/if}
  </div>
</section>

<style>
  .resource-panel {
    container: resources / inline-size;
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    flex: 1;
    overflow: hidden;
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
    color: var(--color-text);
  }

  header {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 12px;
    border-block-end: 1px solid var(--color-border);
  }

  h1 {
    margin: 0;
    font-size: var(--text-body);
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  header > div {
    display: flex;
    gap: 6px;
  }

  .resource-layout {
    display: grid;
    grid-template-columns: minmax(180px, 230px) minmax(0, 1fr);
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .resource-list {
    min-width: 0;
    overscroll-behavior: contain;
    padding: 12px;
    overflow: auto;
    border-inline-end: 1px solid var(--color-border);
  }

  .search {
    display: grid;
    gap: 6px;
    margin-block-end: 12px;
    font-size: var(--text-caption);
    color: var(--color-muted);
  }

  input {
    width: 100%;
    box-sizing: border-box;
    min-width: 0;
    padding: 10px 12px;
    font: inherit;
    background: var(--color-field);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-field);
  }

  input:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  ul {
    list-style: none;
    margin: 12px 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }

  .resource-editor {
    min-width: 0;
    overflow-wrap: anywhere;
    overscroll-behavior: contain;
    padding: 20px;
    overflow: auto;
  }

  .resource-welcome {
    display: grid;
    place-content: center;
    min-width: 0;
    padding: 24px;
  }

  .empty,
  p {
    color: var(--color-muted);
    line-height: 1.5;
    font-size: var(--text-caption);
  }

  .back {
    display: none;
    margin-block-end: 16px;
  }

  @container resources (width < 600px) {
    .resource-layout {
      grid-template-columns: minmax(0, 1fr);
    }

    .resource-list {
      border: 0;
    }

    .editing .resource-list,
    .resource-welcome {
      display: none;
    }

    .back {
      display: block;
    }

    .resource-editor {
      padding: 16px;
    }
  }
</style>
