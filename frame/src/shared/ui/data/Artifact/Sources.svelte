<script lang="ts">
  import { SvelteMap } from "svelte/reactivity";
  import type { EvidenceReference, SourceEntryView, SubjectView } from "./artifact";
  import HostGlyph from "./HostGlyph.svelte";
  let {
    summary,
    subjects,
    entries,
    fallback,
    onevidence,
    limit,
    more,
  }: {
    summary: string;
    subjects: readonly SubjectView[];
    entries: readonly SourceEntryView[];
    fallback: readonly EvidenceReference[];
    onevidence?: (reference: EvidenceReference) => void;
    /** Card mode: one flat list of this many rows, then "+n". */
    limit?: number;
    more?: (count: number) => string;
  } = $props();
  const groups = $derived.by(() => {
    const byKey = new SvelteMap<string, SourceEntryView>();
    for (const entry of entries) byKey.set(entry.evidence.key, entry);
    const rows = fallback.map(
      (reference) =>
        byKey.get(reference.key) ?? {
          evidence: reference,
          title: reference.label,
          role: "",
          subject: undefined,
        },
    );
    const grouped = new SvelteMap<string, SourceEntryView[]>();
    for (const row of rows) {
      const key = row.subject !== undefined ? (subjects[row.subject]?.name ?? "") : "";
      grouped.set(key, [...(grouped.get(key) ?? []), row]);
    }
    if (limit !== undefined) return [["", rows.slice(0, limit)] as [string, SourceEntryView[]]];
    return [...grouped.entries()];
  });
  const total = $derived(fallback.length);
  const shown = $derived(groups.reduce((n, [, rows]) => n + rows.length, 0));
</script>

{#if summary}<p class="summary" class:card={limit !== undefined}>{summary}</p>{/if}
{#each groups as [subject, rows] (subject)}
  {#if subject}<h4>{subject}</h4>{/if}
  <ul class="sources">
    {#each rows as row (row.evidence.key)}
      <li>
        <button type="button" disabled={!onevidence} onclick={() => onevidence?.(row.evidence)}>
          <HostGlyph host={row.evidence.origin || row.title} file={!!row.evidence.file} size={20} />
          <span class="text">
            <span class="title">{row.title}</span>
            <span class="origin">{[row.evidence.origin, row.role].filter(Boolean).join(" · ")}</span
            >
          </span>
        </button>
      </li>
    {/each}
  </ul>
{/each}
{#if more && total > shown}<p class="more">{more(total - shown)}</p>{/if}

<style>
  .summary {
    margin: 0 0 10px;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 17px;
  }

  .summary.card {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }

  .more {
    margin: 4px 0 0 8px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  h4 {
    margin: 10px 0 4px;
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  .sources {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  button {
    display: flex;
    align-items: center;
    gap: 10px;
    inline-size: 100%;
    padding: 6px 8px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-instant) ease;
  }

  button:hover:not(:disabled) {
    background: var(--color-fill-hover);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
  }

  .title,
  .origin {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .title {
    font-size: var(--text-label);
    line-height: 17px;
  }

  .origin {
    color: var(--color-faint);
    font-size: var(--text-caption);
  }
</style>
