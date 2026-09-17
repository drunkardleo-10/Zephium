<script lang="ts">
  import type { TabView } from "$shared/ipc/bindings";
  import FavIcon from "$shared/ui/FavIcon";
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";

  let {
    tabs,
    spaceName,
    currentTabId = null,
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
    /** The tab the Space is on; it reads first, the way a tab strip does. */
    currentTabId?: string | null;
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
    tabs
      .filter((tab) =>
        `${tab.title} ${tab.url ?? ""}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
      )
      .slice()
      .sort((a, b) => Number(b.id === currentTabId) - Number(a.id === currentTabId)),
  );
  const eligible = $derived(
    selected.filter((id) => tabs.some((tab) => tab.id === id) && !attachedTabIds.includes(id)),
  );
  function toggle(tabId: string) {
    selected = selected.includes(tabId)
      ? selected.filter((id) => id !== tabId)
      : [...selected, tabId];
  }
  function host(url: string | null) {
    if (!url) return "";
    try {
      return new URL(url).host;
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
      {@const attached = attachedTabIds.includes(tab.id)}
      <li
        class:chosen={eligible.includes(tab.id)}
        class:current={tab.id === currentTabId}
        class:attached
      >
        <label class="tab">
          <input
            type="checkbox"
            checked={eligible.includes(tab.id)}
            disabled={pending || attached}
            onchange={() => toggle(tab.id)}
          />
          <FavIcon favicon={tab.favicon} />
          <span class="identity">
            <strong>{tab.title || m.work_env_untitled_tab()}</strong>
            <span class="where"
              >{#if tab.id === currentTabId}<span class="badge">{m.work_env_current_tab()}</span
                >{/if}{host(tab.url)}</span
            >
          </span>
        </label>
        {#if attached}<span class="attached-mark">{m.work_env_attached()}</span>{/if}
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
    gap: 10px;
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
  .where,
  .privacy,
  .attached-mark,
  .empty {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  header strong {
    font-weight: 600;
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
    block-size: 30px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
  }

  input:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    max-block-size: min(360px, 45vh);
  }

  /* A row is a tab: its mark, its title, and where it is. */
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px 4px 6px;
    border-radius: var(--radius-control);
    background: var(--color-fill);
    box-shadow: inset 0 0 0 1px transparent;
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      box-shadow var(--motion-fast) var(--ease-smooth);
  }

  li:hover {
    background: var(--color-fill-hover);
  }

  li.current {
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  li.chosen {
    background: var(--color-accent-soft);
    box-shadow: inset 0 0 0 1px var(--color-accent);
  }

  li.attached {
    opacity: 0.6;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-block-size: 32px;
    min-inline-size: 0;
    cursor: default;
  }

  input[type="checkbox"] {
    appearance: none;
    flex-shrink: 0;
    inline-size: 15px;
    block-size: 15px;
    margin: 0;
    border-radius: 50%;
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      box-shadow var(--motion-fast) var(--ease-smooth);
  }

  input[type="checkbox"]:checked {
    background:
      url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 12'%3E%3Cpath d='M2.5 6.2 4.8 8.5 9.5 3.8' fill='none' stroke='black' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E")
        center / 11px no-repeat,
      var(--color-accent);
    box-shadow: none;
  }

  input[type="checkbox"]:disabled {
    box-shadow: inset 0 0 0 1px var(--color-border);
  }

  .identity {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-inline-size: 0;
  }

  .identity strong {
    font-size: var(--text-body);
    font-weight: 500;
  }

  .identity strong,
  .where {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .badge {
    margin-inline-end: 6px;
    padding: 0 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-active);
    color: var(--color-label-secondary);
  }

  .attached-mark {
    flex: none;
  }

  .privacy,
  p {
    margin: 0;
  }

  .empty {
    padding: 8px;
  }

  footer {
    flex-wrap: wrap;
  }
</style>
