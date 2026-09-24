<script lang="ts">
  import type { CellView, CriterionView, EvidenceReference, SubjectView } from "./artifact";
  import EvidenceChips from "./EvidenceChips.svelte";
  let {
    subjects,
    criteria,
    cells,
    notes,
    labels,
    onevidence,
    card = false,
    more,
  }: {
    subjects: readonly SubjectView[];
    criteria: readonly CriterionView[];
    cells: readonly (readonly CellView[])[];
    notes: readonly string[];
    labels: {
      unknown: string;
      generalKnowledge: string;
      criterion: string;
      subject: string;
      yes: string;
      no: string;
    };
    onevidence?: (reference: EvidenceReference) => void;
    /** Card mode: three subjects by three criteria, no sources, then "+n". */
    card?: boolean;
    more?: (count: number) => string;
  } = $props();
  const CARD = 3;
  const shownSubjects = $derived(card ? subjects.slice(0, CARD) : subjects);
  const shownCriteria = $derived(card ? criteria.slice(0, CARD) : criteria);
  const money = (amount: string, currency: string) => {
    const value = Number(amount);
    if (!Number.isFinite(value)) return `${amount} ${currency}`;
    try {
      return new Intl.NumberFormat(undefined, { style: "currency", currency }).format(value);
    } catch {
      return `${amount} ${currency}`;
    }
  };
</script>

<div class="matrix-scroll" class:card tabindex="-1">
  <table class="matrix">
    <thead>
      <tr>
        <th scope="col" class="corner"><span class="sr-only">{labels.criterion}</span></th>
        {#each shownSubjects as subject, index (index)}
          <th scope="col" class="subject">
            <span class="name">{subject.name}</span>
            {#if subject.descriptor && !card}<span class="descriptor">{subject.descriptor}</span
              >{/if}
          </th>
        {/each}
        {#if card && subjects.length > CARD}<th scope="col" class="stub"
            >+{subjects.length - CARD}</th
          >{/if}
      </tr>
    </thead>
    <tbody>
      {#each shownCriteria as criterion, column (column)}
        <tr>
          <th scope="row" class="criterion">
            <span class="name">{criterion.name}</span>
            {#if criterion.kind === "measurement" && criterion.unit}<span class="meta"
                >{criterion.unit}{criterion.basis ? ` · ${criterion.basis}` : ""}</span
              >{:else if criterion.kind === "rating" && criterion.rubric}<span class="meta"
                >{criterion.rubric}</span
              >{/if}
          </th>
          {#each shownSubjects as _, row (row)}
            {@const cell = cells[row]?.[column]}
            <td class:unknown={!cell || cell.value.kind === "unknown"}>
              {#if !cell || cell.value.kind === "unknown"}<span class="value muted"
                  >{labels.unknown}</span
                >{:else if cell.value.kind === "text"}<span class="value">{cell.value.text}</span>
              {:else if cell.value.kind === "measurement"}<span class="value number"
                  >{cell.value.value}{criterion.unit ? ` ${criterion.unit}` : ""}</span
                >
              {:else if cell.value.kind === "money"}<span class="value number"
                  >{money(cell.value.amount, cell.value.currency)}</span
                >
              {:else if cell.value.kind === "rating"}<span class="value rating"
                  ><span class="bar" aria-hidden="true"
                    ><i
                      style:inline-size={`${(cell.value.value / (criterion.scaleMax ?? 5)) * 100}%`}
                    ></i></span
                  ><span class="number">{cell.value.value}/{criterion.scaleMax ?? 5}</span></span
                >
              {:else if cell.value.kind === "presence"}<span
                  class="value presence"
                  class:yes={cell.value.present}>{cell.value.present ? labels.yes : labels.no}</span
                >{/if}
              {#if cell?.note && !card}<span class="note">{cell.note}</span>{/if}
              {#if cell && !card}<span class="basis">
                  {#if cell.generalKnowledge}<span class="general">{labels.generalKnowledge}</span
                    >{/if}
                  <EvidenceChips references={cell.evidence} compact {onevidence} />
                </span>{/if}
            </td>
          {/each}
          {#if card && subjects.length > CARD}<td class="stub"></td>{/if}
        </tr>
      {/each}
    </tbody>
  </table>
</div>
{#if card && criteria.length > CARD && more}<p class="more">{more(criteria.length - CARD)}</p>
{:else if notes.length && !card}<ul class="notes">
    {#each notes as note, index (index)}<li>{note}</li>{/each}
  </ul>{/if}

<style>
  .matrix-scroll {
    overflow: auto;
    max-inline-size: 100%;
  }

  .matrix {
    border-collapse: separate;
    border-spacing: 0;
    min-inline-size: 100%;
    font-size: var(--text-label);
  }

  th,
  td {
    vertical-align: top;
    padding: 10px 12px;
    border-block-end: 1px solid var(--color-border);
    text-align: start;
  }

  thead th {
    position: sticky;
    inset-block-start: 0;
    background: var(--color-surface);
    z-index: 1;
  }

  .corner {
    min-inline-size: 140px;
  }

  .subject {
    min-inline-size: 160px;
  }

  .subject .name,
  .criterion .name {
    display: block;
    font-weight: 600;
    color: var(--color-text);
  }

  .descriptor,
  .meta,
  .note,
  .muted {
    display: block;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 400;
  }

  .criterion {
    position: sticky;
    inset-inline-start: 0;
    background: var(--color-surface);
    min-inline-size: 140px;
    max-inline-size: 220px;
    z-index: 1;
  }

  .value {
    display: block;
    line-height: 17px;
  }

  .number {
    font-variant-numeric: tabular-nums;
  }

  .rating {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .bar {
    display: inline-block;
    inline-size: 56px;
    block-size: 4px;
    border-radius: 2px;
    background: var(--color-track);
    overflow: hidden;
  }

  .bar i {
    display: block;
    block-size: 100%;
    background: var(--color-text);
  }

  .presence {
    color: var(--color-muted);
  }

  .presence.yes {
    color: var(--color-success);
  }

  td.unknown {
    background: color-mix(in srgb, var(--color-fill) 50%, transparent);
  }

  .basis {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    margin-block-start: 6px;
  }

  .general {
    padding: 1px 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-faint);
    font-size: 10px;
  }

  .card .matrix {
    table-layout: fixed;
  }

  .card th,
  .card td {
    padding: 6px 8px;
  }

  .card .corner,
  .card .criterion {
    min-inline-size: 0;
    inline-size: 30%;
  }

  .card .subject {
    min-inline-size: 0;
  }

  .card .name,
  .card .value {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card .meta {
    display: none;
  }

  .stub {
    inline-size: 36px;
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-weight: 400;
  }

  .more {
    margin: 6px 0 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .notes {
    margin: 12px 0 0;
    padding-inline-start: 18px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .sr-only {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
</style>
