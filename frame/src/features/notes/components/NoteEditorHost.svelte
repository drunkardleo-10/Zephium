<script lang="ts">
  import { untrack } from "svelte";
  import { resourceSession, type ResourceSession } from "$domain/resources";
  import SaveStatus from "$shared/ui/data/SaveStatus";
  import LazyView from "$shared/ui/LazyView";
  import * as m from "$shared/i18n/messages";
  let {
    profile,
    host,
    id,
    autofocus = false,
  }: { profile: string; host: string; id: string; autofocus?: boolean } = $props();
  let session = $state.raw<ResourceSession | null>(null);
  const loadEditor = () => import("./NotesEditor.svelte");
  let titleInput = $state<HTMLInputElement>();
  $effect(() => {
    const owner = profile;
    const scope = host;
    const target = id;
    const current = untrack(() => resourceSession(owner, "note", scope));
    session = current;
    if (!current) return;
    let live = true;
    void current.start().then(async () => {
      if (!live) return;
      if (current.record?.id !== target) await current.open(target);
      if (live && autofocus && current.draft && current.draft.title === m.note_untitled()) {
        titleInput?.focus();
        titleInput?.select();
      }
    });
    return () => {
      live = false;
      void current.flush();
      current.stopObserving();
    };
  });
  export async function flush() {
    return session ? session.flush() : true;
  }
</script>

{#if session?.draft?.content.kind === "note" && session.record?.id === id}{@const current = session}
  <div class="host">
    <header>
      <input
        bind:this={titleInput}
        class="title"
        value={current.draft!.title}
        maxlength="256"
        required
        aria-label={m.resource_title()}
        disabled={!current.canEdit}
        oninput={(event) => current.edit({ title: event.currentTarget.value })}
      />
      <div class="status">
        {#if current.draft || current.pending}<SaveStatus
            state={current.saveState}
            onretry={() => void current.retry()}
            ondiscard={() => void current.discardDraft()}
            onkeep={() => void current.keepDraft()}
          />{/if}
      </div>
    </header>
    <div class="body">
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
              onopen={(target) => void current.open(target)}
              findNotes={(query) => current.findNotes(query)}
              resolveNotes={(ids) => current.resolveNotes(ids)}
              referencesRevision={current.referencesRevision}
            />{/snippet}</LazyView
        >{/key}
    </div>
  </div>
{:else}<p class="loading" role="status">{m.surface_loading()}</p>{/if}

<style>
  .host {
    display: flex;
    flex-direction: column;
    min-block-size: 0;
    block-size: 100%;
  }

  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 0 8px;
  }

  .title {
    flex: 1;
    min-inline-size: 0;
    border: 0;
    padding: 4px 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-title);
    font-weight: 600;
    letter-spacing: -0.01em;
    outline: none;
  }

  .title:focus-visible {
    box-shadow: 0 2px var(--color-ring);
  }

  .status {
    flex: none;
    min-inline-size: 0;
  }

  .body {
    flex: 1;
    min-block-size: 0;
    overflow: auto;
  }

  .loading {
    margin: 0;
    color: var(--color-muted);
  }
</style>
