<script lang="ts">
  import { SvelteMap } from "svelte/reactivity";
  import type { EvidenceReference, SourceEntryView, SubjectView } from "./artifact";
  let {
    summary,
    subjects,
    entries,
    fallback,
    onevidence,
  }: {
    summary: string;
    subjects: readonly SubjectView[];
    entries: readonly SourceEntryView[];
    fallback: readonly EvidenceReference[];
    onevidence?: (reference: EvidenceReference) => void;
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
    return [...grouped.entries()];
  });
</script>

{#if summary}<p class="summary">{summary}</p>{/if}
{#each groups as [subject, rows] (subject)}
  {#if subject}<h4>{subject}</h4>{/if}
  <ul class="sources">
    {#each rows as row (row.evidence.key)}
      <li>
        <button type="button" disabled={!onevidence} onclick={() => onevidence?.(row.evidence)}>
          <span class="glyph" aria-hidden="true">{(row.evidence.origin || "•").slice(0, 1)}</span>
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

<style>
  .summary {
    margin: 0 0 10px;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 17px;
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

  .glyph {
    display: grid;
    place-items: center;
    flex: none;
    inline-size: 22px;
    block-size: 22px;
    border-radius: 6px;
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font-size: var(--text-caption);
    font-weight: 600;
    text-transform: uppercase;
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
