<script lang="ts">
  import type { TabView } from "$shared/ipc/bindings";
  import FavIcon from "$shared/ui/FavIcon";
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";

  let {
    tabs,
    spaceName,
    attachedTabIds = [],
    pending = false,
    status = "",
    onattach,
    onopen,
    onnewtab,
  }: {
    /** The current Space's authoritative tab projection, never a URL-derived list. */
    tabs: readonly TabView[];
    spaceName: string;
    attachedTabIds?: readonly TabView["id"][];
    pending?: boolean;
    status?: string;
    onattach: (ids: TabView["id"][]) => void;
    onopen: (id: TabView["id"]) => void;
    onnewtab: () => void;
  } = $props();
  const id = $props.id();
  let query = $state("");
  let selected = $state<string[]>([]);
  const visible = $derived(
    tabs.filter((tab) =>
      `${tab.title} ${tab.url ?? ""}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
    ),
  );
  const eligible = $derived(
    selected.filter((id) => tabs.some((tab) => tab.id === id) && !attachedTabIds.includes(id)),
  );
  function toggle(tabId: string) {
    selected = selected.includes(tabId)
      ? selected.filter((id) => id !== tabId)
      : [...selected, tabId];
  }
  function origin(url: string | null) {
    if (!url) return "";
    try {
      return new URL(url).origin === "null" ? "" : new URL(url).origin;
    } catch {
      return "";
    }
  }
</script>

<section class="picker" aria-label={m.work_env_tabs()} aria-busy={pending}>
  <header><strong>{m.work_env_tabs()}</strong><span>{spaceName}</span></header>
  <label class="search" for={id}>{m.work_env_search_tabs()}</label>
  <input {id} type="search" bind:value={query} placeholder={m.work_env_search_tabs()} />
  <ul>
    {#each visible as tab (tab.id)}
      <li>
        <label class="tab">
          <input
            type="checkbox"
            checked={eligible.includes(tab.id)}
            disabled={pending || attachedTabIds.includes(tab.id)}
            onchange={() => toggle(tab.id)}
          />
          <FavIcon favicon={tab.favicon} />
          <span class="identity"
            ><strong>{tab.title || m.work_env_untitled_tab()}</strong><span>{origin(tab.url)}</span
            ></span
          >
        </label>
        {#if attachedTabIds.includes(tab.id)}<span class="attached">{m.work_env_attached()}</span
          >{/if}
        <Button
          size="compact"
          disabled={pending}
          aria-label={m.work_env_open_tab({ title: tab.title || m.work_env_untitled_tab() })}
          onclick={() => onopen(tab.id)}>{m.work_env_open_here()}</Button
        >
      </li>
    {:else}<li class="empty">{query ? m.work_env_no_matches() : m.work_env_no_tabs()}</li>{/each}
  </ul>
  <p class="privacy">{m.work_env_tabs_privacy()}</p>
  {#if status}<p role="status">{status}</p>{/if}
  <footer>
    <Button size="compact" disabled={pending} onclick={onnewtab}>{m.work_env_new_tab()}</Button
    ><Button
      size="compact"
      disabled={pending || eligible.length === 0}
      onclick={() => onattach([...eligible])}
      >{pending ? m.work_env_pending() : m.work_env_add_tabs({ count: eligible.length })}</Button
    >
  </footer>
</section>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-inline-size: 0;
  }

  header,
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  header span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  header span,
  .identity span,
  .privacy,
  .attached,
  .empty {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  strong {
    font-weight: 500;
  }

  .search {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  input[type="search"] {
    inline-size: 100%;
    box-sizing: border-box;
    padding: 8px 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
  }

  input:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    max-block-size: min(360px, 45vh);
  }

  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-block: 8px;
    border-block-end: 1px solid var(--color-border);
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-inline-size: 0;
    cursor: pointer;
  }

  input[type="checkbox"] {
    flex-shrink: 0;
    accent-color: var(--color-accent);
  }

  .identity {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-inline-size: 0;
  }

  .identity strong,
  .identity span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .privacy,
  p {
    margin: 0;
  }

  footer {
    flex-wrap: wrap;
  }
</style>
