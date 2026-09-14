<script lang="ts">
  import DataTable from "../DataTable";
  import LazyView from "$shared/ui/LazyView";
  import Button from "$shared/ui/Button";
  import { loadChart } from "../Chart";
  import {
    artifactRenderable,
    displayLocation,
    type ArtifactView,
    type EvidenceReference,
  } from "./artifact";
  import * as m from "$shared/i18n/messages";
  let {
    artifact,
    onevidence,
    embedded = false,
  }: {
    artifact: ArtifactView;
    onevidence?: (reference: EvidenceReference) => void;
    embedded?: boolean;
  } = $props();
  let valid = $derived(artifactRenderable(artifact));
  let content = $derived(artifact.content);
  const labels = {
    rowHeading: m.work_table_row(),
    actions: m.work_actions(),
    empty: m.work_empty_data(),
    missing: "—",
    unavailable: m.work_artifact_unavailable(),
    previous: m.work_previous(),
    next: m.work_next(),
    range: (first: number, last: number, total: number) =>
      m.work_table_range({ first, last, total }),
  };
</script>

<article aria-label={artifact.title} class="artifact">
  {#if !embedded}<header>
      <h2>{artifact.title}</h2>
      <p class="review">{artifact.reviewLabel}</p>
    </header>{/if}
  {#if !valid}<p role="alert">{m.work_artifact_unavailable()}</p>
  {:else if content.kind === "document"}<div class="document">
      {#each content.paragraphs as paragraph, i (i)}<p>{paragraph}</p>{:else}<p>
          {m.work_empty_data()}
        </p>{/each}
    </div>
  {:else if content.kind === "table"}<DataTable
      caption={artifact.title}
      showCaption={!embedded}
      columns={content.columns.map((label, i) => ({ key: String(i), label }))}
      rows={content.rows.map((row, i) => ({
        key: String(i),
        label: String(i + 1),
        cells: Object.fromEntries(row.map((cell, c) => [String(c), cell])),
      }))}
      {labels}
    />
  {:else if content.kind === "comparison"}<DataTable
      caption={artifact.title}
      showCaption={!embedded}
      columns={content.criteria.map((label, i) => ({ key: String(i), label }))}
      rows={content.alternatives.map((row, i) => ({
        key: String(i),
        label: row.name,
        cells: Object.fromEntries(row.values.map((cell, c) => [String(c), cell])),
      }))}
      {labels}
    />
  {:else if content.kind === "chart"}<LazyView
      loader={loadChart}
      loadingLabel={m.surface_loading()}
      failureLabel={m.work_artifact_unavailable()}
      retryLabel={m.surface_retry()}
      >{#snippet children(Chart)}<Chart
          title={artifact.title}
          xLabel={content.xLabel}
          yLabel={content.yLabel}
          series={content.series}
        />{/snippet}</LazyView
    >
  {:else if content.kind === "checklist"}<ul class="checklist">
      {#each content.items as item, i (i)}<li>
          <span aria-label={item.completed ? m.work_item_complete() : m.work_item_open()}
            >{item.completed ? "✓" : "○"}</span
          ><span>{item.text}</span>
        </li>{:else}<li>{m.work_empty_data()}</li>{/each}
    </ul>
  {:else if content.kind === "sources"}<p>{content.summary}</p>
  {:else if content.kind === "browser"}<section class="resource">
      <p class="eyebrow">{m.work_browser_resource()}</p>
      <h3>{content.title}</h3>
      <p class="location">{displayLocation(content.location) || m.work_location_unavailable()}</p>
      <p>{content.summary}</p>
      <small>{m.work_browser_preview_only()}</small>
    </section>
  {:else if content.kind === "unavailable"}<p role="status">{content.reason}</p>{/if}
  {#if valid && artifact.evidence.length}<footer aria-label={m.work_sources()}>
      {#each artifact.evidence as reference (reference.key)}<Button
          size="compact"
          disabled={!onevidence}
          onclick={() => onevidence?.(reference)}>{reference.label}</Button
        >{/each}
    </footer>{/if}
</article>

<style>
  .artifact {
    min-width: 0;
    color: var(--color-text);
    overflow-wrap: anywhere;
  }

  header {
    margin-block-end: 20px;
  }

  h2 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
  }

  h3 {
    margin-block: 8px;
    font-size: inherit;
  }

  p {
    line-height: 1.65;
    white-space: pre-wrap;
  }

  .review,
  .eyebrow,
  small {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .document {
    max-width: 72ch;
  }

  .document p {
    margin-block: 0 16px;
  }

  .checklist {
    padding: 0;
    list-style: none;
  }

  .checklist li {
    display: flex;
    gap: 12px;
    padding-block: 8px;
  }

  .resource {
    padding: 20px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-fill);
  }

  .location {
    color: var(--color-muted);
  }

  footer {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-block-start: 24px;
    padding-block-start: 16px;
    border-block-start: 1px solid var(--color-border);
  }
</style>
