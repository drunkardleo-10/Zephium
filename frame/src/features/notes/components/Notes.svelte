<script lang="ts">
  import { untrack } from "svelte";
  import { commands } from "$shared/ipc/bindings";
  import { resourceSession, type ResourceSession } from "$domain/resources";
  import ResourcePanel from "$shared/ui/data/ResourcePanel";
  import SaveStatus from "$shared/ui/data/SaveStatus";
  import Button from "$shared/ui/Button";
  import LazyView from "$shared/ui/LazyView";
  import NoteCard from "./NoteCard.svelte";
  import * as m from "$shared/i18n/messages";
  let {
    profile,
    host = "work",
    onclose,
    ondrag,
    onback,
  }: {
    profile: string;
    host?: string;
    onclose?: () => void;
    ondrag?: () => void;
    onback?: () => void;
  } = $props();
  let session = $state.raw<ResourceSession | null>(
    untrack(() => resourceSession(profile, "note", host)),
  );
  const loadEditor = () => import("./NotesEditor.svelte");
  $effect(() => {
    const owner = profile;
    const scope = host;
    const current = untrack(() => resourceSession(owner, "note", scope));
    session = current;
    void current?.start();
    return () => current?.stopObserving();
  });
  let titleInput = $state<HTMLInputElement>();
  async function create() {
    const current = session;
    await current?.create(m.note_untitled());
    if (current === session && current?.record && current.saveState === "saved") {
      titleInput?.focus();
      titleInput?.select();
    }
  }
  async function close() {
    const current = session;
    if (current && (await current.flush()) && current === session) onclose?.();
  }
  async function exit() {
    const current = session;
    if (current && (await current.flush()) && current === session) onback?.();
  }
  async function trashFilter() {
    const current = session;
    if (current && (await current.flush()) && current === session) {
      await current.back();
      if (current !== session) return;
      current.trash = !current.trash;
      void current.reload();
    }
  }
</script>

{#if session}<ResourcePanel
    title={m.tool_notes()}
    rows={session.items}
    query={session.query}
    onquery={(value) => session?.search(value)}
    loading={session.loading}
    error={session.error}
    editing={!!session.draft}
    oncreate={create}
    onclose={onclose ? close : undefined}
    onback={() => {
      void session?.back();
    }}
    {ondrag}
    onexit={onback ? exit : undefined}
    hasMore={!!session.next}
    onmore={() => {
      void session?.reload(true);
    }}
    trash={session.trash}
    ontrash={trashFilter}
  >
    {#snippet status()}{#if session && (session.draft || session.pending)}<SaveStatus
          state={session.saveState}
          onretry={() => {
            void session?.retry();
          }}
          ondiscard={() => {
            void session?.discardDraft();
          }}
          onkeep={() => {
            void session?.keepDraft();
          }}
        />{/if}{/snippet}
    {#snippet row(note)}<NoteCard
        title={note.title}
        pinned={note.pinned}
        selected={session?.record?.id === note.id}
        onopen={() => {
          void session?.open(note.id);
        }}
      />{/snippet}
    {#snippet editor()}{#if session?.draft?.content.kind === "note"}{@const current = session}
        <label class="note-title"
          ><span>{m.resource_title()}</span><input
            bind:this={titleInput}
            value={current.draft!.title}
            maxlength="256"
            required
            disabled={!current.canEdit}
            oninput={(event) => current.edit({ title: event.currentTarget.value })}
          /></label
        >
        <div class="note-actions">
          <Button
            variant="ghost"
            size="compact"
            disabled={!current.canEdit}
            aria-pressed={current.draft!.pinned}
            onclick={() => current.edit({ pinned: !current.draft!.pinned })}
            >{m.resource_pin()}</Button
          >{#if current.record}<Button
              size="compact"
              disabled={current.navigating}
              onclick={() => {
                void current.setTrashed(!current.record!.trashed);
              }}>{current.record.trashed ? m.resource_restore() : m.resource_trash()}</Button
            >{/if}
        </div>
        {#key `${profile}:${current.editorKey}`}<LazyView
            loader={loadEditor}
            loadingLabel={m.surface_loading()}
            failureLabel={m.surface_render_failed()}
            retryLabel={m.surface_retry()}
            >{#snippet children(Editor)}<Editor
                value={current.draft!.content.kind === "note"
                  ? current.draft!.content.document
                  : { version: 1, document: { type: "doc", content: [{ type: "paragraph" }] } }}
                disabled={!current.canEdit}
                onchange={(document) => current.edit({ content: { kind: "note", document } })}
                onopen={(id) => {
                  void current.open(id);
                }}
                onlink={(href) => void commands.tabsOpenUrl(href)}
                findNotes={(query) => current.findNotes(query)}
                resolveNotes={(ids) => current.resolveNotes(ids)}
                referencesRevision={current.referencesRevision}
              />{/snippet}</LazyView
          >{/key}
      {/if}{/snippet}
  </ResourcePanel>{:else}<p role="alert">{m.resource_draft_capacity()}</p>{/if}

<style>
  .note-title {
    display: grid;
    gap: 8px;
  }

  .note-title span {
    font-size: var(--text-caption);
    color: var(--color-muted);
  }

  input {
    width: 100%;
    box-sizing: border-box;
    font: inherit;
    font-size: var(--text-title);
    font-weight: 600;
    color: var(--color-text);
    background: transparent;
    border: 0;
    padding: 8px 0;
    outline: none;
  }

  input:focus-visible {
    box-shadow: 0 2px var(--color-ring);
  }

  .note-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-block: 12px;
  }
</style>
