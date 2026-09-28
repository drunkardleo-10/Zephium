<script lang="ts">
  import { tick } from "svelte";
  import * as m from "$shared/i18n/messages";
  import { tabs } from "$domain/tabs";
  import { WorkMemorySession } from "$domain/work-context";
  import type { WorkMemoryKindV1, WorkMemoryRefusalV1, WorkMemoryV1 } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
  import SearchField from "$shared/ui/SearchField";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import Add01Icon from "@hugeicons/core-free-icons/Add01Icon";
  import Cancel01Icon from "@hugeicons/core-free-icons/Cancel01Icon";
  import { sinceLabel } from "../../lib/work-context";

  const profile = $derived(tabs.profile());
  let session = $state.raw<WorkMemorySession | null>(null);
  $effect(() => {
    const id = profile && profile.kind !== "incognito" ? profile.id : null;
    if (!id) return;
    const owner = new WorkMemorySession(id);
    session = owner;
    void owner.start();
    return () => owner.dispose();
  });

  const KINDS: Record<WorkMemoryKindV1, () => string> = {
    preference: m.settings_memory_preference,
    person: m.settings_memory_person,
    project: m.settings_memory_project,
    fact: m.settings_memory_fact,
  };
  const REFUSED: Record<WorkMemoryRefusalV1, () => string> = {
    empty: m.settings_memory_refused_empty,
    too_long: m.settings_memory_refused_long,
    lines: m.settings_memory_refused_lines,
    secret: m.settings_memory_refused_secret,
  };

  let query = $state("");
  let searching = $state(false);
  let searched = "";
  $effect(() => {
    const text = query.trim();
    const owner = session;
    if (!owner || text === searched) return;
    const timer = setTimeout(() => {
      searched = text;
      void owner.search(text);
    }, 180);
    return () => clearTimeout(timer);
  });
  $effect(() => {
    if ((session?.memories.length ?? 0) > 8) searching = true;
  });

  let adding = $state("");
  async function add() {
    const text = adding.trim();
    if (!session || !text) return;
    if (await session.change({ kind: "add", text, memory: "fact" })) adding = "";
  }

  /** The fact open for editing, and its words as they stand. */
  let editing = $state<string | null>(null);
  let draft = $state("");
  let field = $state<HTMLInputElement>();
  async function edit(memory: WorkMemoryV1) {
    editing = memory.id;
    draft = memory.text;
    await tick();
    field?.focus();
    field?.select();
  }
  async function save(memory: WorkMemoryV1) {
    if (editing !== memory.id) return;
    const text = draft.trim();
    editing = null;
    if (!session || !text || text === memory.text) return;
    if (await session.change({ kind: "edit", id: memory.id, text, memory: memory.kind })) return;
    // Refused: the words stay open to be put right.
    editing = memory.id;
    draft = text;
  }

  let clearing = $state(false);
  const source = (memory: WorkMemoryV1) =>
    memory.work
      ? memory.source
        ? m.settings_memory_from_work({ work: memory.source })
        : m.settings_memory_from_a_work()
      : m.settings_memory_you_added();
</script>

{#if !profile || profile.kind === "incognito"}
  <p class="memory-note">{m.work_regular_profile()}</p>
{:else}
  {#if searching}<div class="search">
      <SearchField
        label={m.settings_memory_search()}
        placeholder={m.settings_memory_search()}
        bind:value={query}
      />
    </div>{/if}
  <SettingsGroup title={m.settings_memory_title()} description={m.settings_memory_help()}>
    {#if session?.loaded && session.memories.length === 0}
      <p class="empty">{query ? m.settings_memory_no_match() : m.settings_memory_empty()}</p>
    {/if}
    {#each session?.memories ?? [] as memory (memory.id)}
      <div class="fact" class:editing={editing === memory.id}>
        {#if editing === memory.id}
          <form
            onsubmit={(event) => {
              event.preventDefault();
              void save(memory);
            }}
          >
            <label class="sr-only" for={`memory-${memory.id}`}>{m.settings_memory_edit()}</label>
            <input
              id={`memory-${memory.id}`}
              bind:this={field}
              bind:value={draft}
              maxlength="280"
              disabled={session?.busy}
              onkeydown={(event) => {
                if (event.key === "Escape") {
                  event.stopPropagation();
                  editing = null;
                }
              }}
              onblur={() => void save(memory)}
            />
          </form>
        {:else}
          <button
            type="button"
            class="words"
            title={m.settings_memory_edit()}
            onclick={() => void edit(memory)}
          >
            <span class="text">{memory.text}</span>
            <span class="about"
              >{KINDS[memory.kind]()} · {source(memory)} · {memory.used_ms
                ? m.settings_memory_used({ when: sinceLabel(memory.used_ms) })
                : sinceLabel(memory.created_ms)}</span
            >
          </button>
          <IconButton
            icon={Cancel01Icon}
            size={14}
            buttonSize={26}
            class="forget"
            label={m.settings_memory_forget()}
            disabled={session?.busy}
            onclick={() => void session?.change({ kind: "forget", id: memory.id })}
          />
        {/if}
      </div>
    {/each}
    <form
      class="add"
      onsubmit={(event) => {
        event.preventDefault();
        void add();
      }}
    >
      <span class="mark" aria-hidden="true"><Icon icon={Add01Icon} size={15} /></span>
      <label class="sr-only" for="work-memory-add">{m.settings_memory_add_label()}</label>
      <input
        id="work-memory-add"
        placeholder={m.settings_memory_add_placeholder()}
        maxlength="280"
        autocomplete="off"
        bind:value={adding}
      />
      <Button type="submit" size="compact" disabled={!adding.trim() || session?.busy}
        >{m.settings_memory_add()}</Button
      >
    </form>
  </SettingsGroup>
  {#if session?.refused}<p class="memory-note warn" role="alert">
      {REFUSED[session.refused]()}
    </p>{:else if session?.unavailable}<p class="memory-note" role="alert">
      {m.settings_memory_unavailable()}
    </p>{/if}
  {#if (session?.memories.length ?? 0) > 0 && !query}
    <div class="clear">
      {#if clearing}
        <span>{m.settings_memory_clear_confirm({ count: session?.memories.length ?? 0 })}</span>
        <Button size="compact" variant="ghost" onclick={() => (clearing = false)}
          >{m.settings_memory_keep()}</Button
        >
        <Button
          size="compact"
          variant="danger"
          disabled={session?.busy}
          onclick={async () => {
            if (await session?.change({ kind: "forget_all" })) clearing = false;
          }}>{m.settings_memory_clear()}</Button
        >
      {:else}
        <Button size="compact" variant="ghost" onclick={() => (clearing = true)}
          >{m.settings_memory_clear_all()}</Button
        >
      {/if}
    </div>
  {/if}
{/if}

<style>
  .search {
    margin: 0 0 18px;
  }

  .fact {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    box-sizing: border-box;
    min-block-size: var(--row-page);
    padding: 6px 12px 6px 6px;
  }

  .fact + .fact::before,
  .add::before {
    content: "";
    position: absolute;
    inset-inline: 18px 0;
    inset-block-start: 0;
    block-size: 1px;
    background: var(--color-border);
  }

  .words {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    min-inline-size: 0;
    padding: 8px 12px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .words:hover {
    background: var(--row-hover);
  }

  .words:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .text {
    color: var(--color-text);
    font-size: var(--text-page-title);
    line-height: 19px;
    overflow-wrap: anywhere;
  }

  .about {
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
  }

  /* Forgetting is there when the row is looked at, not a column of crosses. */
  .fact :global(.forget) {
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .fact:hover :global(.forget),
  .fact :global(.forget:focus-visible) {
    opacity: 1;
  }

  .editing form {
    flex: 1;
    margin: 0;
  }

  .editing input,
  .add input {
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 36px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-row);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-page-title);
    outline: none;
  }

  .add {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 0;
    padding: 10px 12px 10px 18px;
  }

  .add input {
    flex: 1;
    min-inline-size: 0;
    block-size: 28px;
    padding: 0;
    border-radius: 0;
    background: transparent;
  }

  .editing input:focus-visible {
    box-shadow: var(--shadow-field-focus);
  }

  .add input::placeholder {
    color: var(--color-faint);
  }

  .mark {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 20px;
    block-size: 20px;
    color: var(--color-muted);
  }

  .empty {
    margin: 0;
    padding: 16px 18px 12px;
    color: var(--color-muted);
    font-size: var(--text-body);
    line-height: 1.5;
  }

  .memory-note {
    margin: -26px 16px 28px;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 1.5;
  }

  .memory-note.warn {
    color: var(--color-warning);
  }

  .clear {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    margin: -18px 8px 28px;
    color: var(--color-muted);
    font-size: var(--text-label);
  }
</style>
