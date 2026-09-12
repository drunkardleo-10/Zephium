<script lang="ts">
  import { loadWorkSurface, type WorkRequestView, type WorkSurfaceIntent } from "$features/work";
  import { loadDocumentEditor } from "$shared/ui/data/DocumentEditor";
  import LazyView from "$shared/ui/LazyView";
  import Evidence from "$shared/ui/data/Evidence";
  import Button from "$shared/ui/Button";
  import { scenarios } from "./scenarios";
  let selected = $state(0);
  let active = $state(true);
  let theme = $state("dark");
  let evidence = $state(false);
  let editing = $state(false);
  let paragraphs = $state([
    "A draft belongs to its author. Editing this example makes no native request.",
  ]);
  let request = $state<WorkRequestView>({
    state: "ready",
    message: "Development data · no native actions or persistence",
  });
  let response = $state<WorkRequestView["state"]>("rejected");
  function intent(_intent: WorkSurfaceIntent) {
    request = {
      state: response,
      message: {
        ready: "No durable change has been claimed.",
        pending: "Awaiting a response. Admission is not completion.",
        rejected: "The example request was rejected. The current projection is unchanged.",
        conflict: "This example revision is stale. Fetch current facts before submitting again.",
        unknown: "The outcome is unknown. Reconcile this request before retrying.",
        resynchronizing: "Refreshing authoritative state; commands are unavailable.",
      }[response],
    };
  }
  $effect(() => {
    document.documentElement.dataset.theme = theme;
  });
</script>

<div class="preview">
  <nav aria-label="Development preview controls">
    <strong>Zephium · Work preview</strong><label
      >Scenario<select
        bind:value={selected}
        onchange={() => {
          request = {
            state: "ready",
            message: "Development data · no native actions or persistence",
          };
          evidence = false;
        }}
        >{#each scenarios as scene, i (scene.key)}<option value={i}>{scene.title}</option
          >{/each}</select
      ></label
    ><label
      >Response<select bind:value={response}
        >{#each ["rejected", "conflict", "unknown", "pending", "resynchronizing"] as value (value)}<option
            >{value}</option
          >{/each}</select
      ></label
    ><label
      >Appearance<select bind:value={theme}
        ><option value="dark">Dark</option><option value="light">Light</option></select
      ></label
    ><Button size="compact" onclick={() => (active = !active)}
      >{active ? "Hide Work" : "Show Work"}</Button
    ><Button size="compact" onclick={() => (editing = !editing)}>Document draft</Button>
  </nav>
  {#if active && editing}<section class="draft">
      <h2>Document draft · not saved</h2>
      <LazyView
        loader={loadDocumentEditor}
        loadingLabel="Loading editor"
        failureLabel="Editor unavailable"
        retryLabel="Retry"
        >{#snippet children(Editor)}<Editor
            {paragraphs}
            label="Document draft"
            onedit={(next) => (paragraphs = next)}
          />{/snippet}</LazyView
      >
    </section>{/if}
  {#if active && evidence}<aside class="evidence">
      <Button size="compact" onclick={() => (evidence = false)}>Close source</Button>
      <Evidence
        evidence={{
          state: "ready",
          title: "Source 1",
          origin: "sqlite.org",
          role: "Documentation",
          text: "Example captured source text. This is fixture content, not a quotation from the live site.",
          truncated: true,
          sourceBytes: "12400",
        }}
      />
    </aside>{/if}
  {#key selected}<LazyView
      loader={loadWorkSurface}
      loadingLabel="Loading Work"
      failureLabel="Work unavailable"
      retryLabel="Retry"
      >{#snippet children(Work)}<Work
          view={scenarios[selected]!}
          {active}
          {request}
          onintent={intent}
          onrefresh={() => {
            request = {
              state: "resynchronizing",
              message: "Preview paused at resynchronization; choose a scenario to reset.",
            };
          }}
          onevidence={() => (evidence = true)}
        />{/snippet}</LazyView
    >{/key}
</div>

<style>
  .preview {
    height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--color-canvas);
    color: var(--color-text);
  }

  nav {
    flex-shrink: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
    padding: 12px 24px;
    border-block-end: 1px solid var(--color-border);
    background: var(--color-chrome);
  }

  nav strong {
    margin-inline-end: auto;
    font-size: var(--text-caption);
  }

  label {
    display: grid;
    gap: 4px;
    font-size: var(--text-caption);
    color: var(--color-muted);
  }

  select {
    max-width: 260px;
    padding: 6px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font: inherit;
    color: var(--color-text);
    background: var(--color-surface);
  }

  .draft,
  .evidence {
    max-height: 50vh;
    overflow: auto;
    flex-shrink: 0;
    padding: 24px;
    border-block-end: 1px solid var(--color-border);
  }

  h2 {
    font-size: var(--text-title);
  }
</style>
