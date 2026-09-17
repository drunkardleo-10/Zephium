<script lang="ts">
  import { mediaUrl } from "$domain/resources";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import HostGlyph from "../cards/HostGlyph.svelte";
  import Icon from "$shared/ui/Icon";
  import { Cancel01Icon, Tick02Icon } from "../../lib/icons";
  import { cellText, type CompareCell, type CompareModel } from "../../lib/compare";
  import * as m from "$shared/i18n/messages";
  let {
    model,
    correctable = false,
    onevidence,
    oncorrect,
  }: {
    model: CompareModel;
    /** Whether this result can still be corrected by the person reading it. */
    correctable?: boolean;
    /** A chip opens its source: the pane for a page, the lift for a file. */
    onevidence?: (reference: EvidenceReference) => void;
    /** Enter commits the cell the person retyped; Escape leaves it as it was. */
    oncorrect?: (cell: CompareCell, text: string) => void;
  } = $props();
  let editing = $state<string | null>(null);
  let draft = $state("");
  const at = (cell: CompareCell) => `${cell.subject}:${cell.criterion}`;
  function start(cell: CompareCell) {
    editing = at(cell);
    draft = cellText(cell.value, m.work_yes(), m.work_no());
  }
  function commit(cell: CompareCell) {
    const text = draft;
    editing = null;
    if (text.trim() !== cellText(cell.value, m.work_yes(), m.work_no()).trim())
      oncorrect?.(cell, text);
  }
</script>

<div class="compare" tabindex="-1">
  <table>
    <thead>
      <tr>
        <th scope="col" class="corner"><span class="sr-only">{m.work_matrix_criterion()}</span></th>
        {#each model.columns as column (column.key)}
          <th scope="col" class="subject">
            <span class="picture">
              {#if column.picture}<img
                  src={mediaUrl(column.picture.profile, column.picture.digest)}
                  alt=""
                />{:else}<span class="mark" aria-hidden="true">{column.name.slice(0, 1)}</span>{/if}
            </span>
            <span class="name">{column.name}</span>
            {#if column.price}<span class="price">{column.price}</span>{/if}
            {#if column.descriptor}<span class="descriptor">{column.descriptor}</span>{/if}
          </th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#each model.rows as row (row.key)}
        <tr>
          <th scope="row" class="criterion">
            <span class="label">{row.label}</span>
            {#if row.meta}<span class="meta">{row.meta}</span>{/if}
          </th>
          {#each row.cells as cell (cell.subject)}
            <td class:numeric={row.numeric} class:unknown={cell.value.kind === "unknown"}>
              <span class="cell">
                {#if editing === at(cell)}
                  <!-- svelte-ignore a11y_autofocus -->
                  <input
                    class="editor"
                    autofocus
                    bind:value={draft}
                    aria-label={`${row.label}, ${model.columns[cell.subject]?.name ?? ""}`}
                    maxlength="512"
                    onkeydown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        commit(cell);
                      } else if (event.key === "Escape") {
                        event.preventDefault();
                        editing = null;
                      }
                    }}
                    onblur={() => (editing = null)}
                  />
                {:else if cell.value.kind === "unknown"}<span class="dash" aria-hidden="true"
                    >—</span
                  ><span class="sr-only">{m.work_cell_unknown()}</span>
                {:else if cell.value.kind === "mark"}<span
                    class="glyph"
                    class:yes={cell.value.yes}
                    aria-label={cell.value.yes ? m.work_yes() : m.work_no()}
                    ><Icon icon={cell.value.yes ? Tick02Icon : Cancel01Icon} size={15} /></span
                  >
                {:else}<span class="text">{cell.value.text}</span>{/if}
                {#if cell.note}<span class="note">{cell.note}</span>{/if}
                <span class="aside">
                  {#if cell.generalKnowledge}<span class="general"
                      >{m.work_general_knowledge()}</span
                    >{/if}
                  {#each cell.evidence as reference (reference.key)}
                    <button
                      type="button"
                      class="chip"
                      title={reference.label}
                      onclick={() => onevidence?.(reference)}
                    >
                      <HostGlyph host={reference.origin ?? ""} file={!!reference.file} size={14} />
                      <span>{reference.origin || reference.label}</span>
                    </button>
                  {/each}
                  {#if correctable && editing !== at(cell)}<button
                      type="button"
                      class="correct"
                      onclick={() => start(cell)}>{m.work_env_correct()}</button
                    >{/if}
                </span>
              </span>
            </td>
          {/each}
        </tr>
      {/each}
    </tbody>
  </table>
  {#if model.notes.length}
    <ul class="notes">
      {#each model.notes as note, index (index)}<li>{note}</li>{/each}
    </ul>
  {/if}
</div>

<style>
  .compare {
    min-inline-size: 0;
    overflow: auto;
  }

  table {
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
    z-index: 2;
    background: var(--color-surface);
    border-block-end: 1px solid var(--color-border-strong);
  }

  .corner {
    inline-size: 150px;
    min-inline-size: 150px;
  }

  .subject {
    min-inline-size: 168px;
    max-inline-size: 240px;
  }

  .picture {
    display: block;
    inline-size: 84px;
    block-size: 84px;
    margin-block-end: 8px;
    border-radius: var(--radius-sm);
    background: var(--color-fill);
    overflow: hidden;
  }

  .picture img {
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
  }

  .mark {
    display: grid;
    place-items: center;
    inline-size: 100%;
    block-size: 100%;
    background: var(--color-accent-soft);
    color: var(--color-text);
    font-size: var(--text-title);
    font-weight: 700;
    text-transform: uppercase;
  }

  .name {
    display: block;
    font-weight: 600;
    line-height: 17px;
  }

  .price {
    display: block;
    margin-block-start: 2px;
    font-size: var(--text-body);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .criterion {
    position: sticky;
    inset-inline-start: 0;
    z-index: 1;
    background: var(--color-surface);
  }

  .label {
    display: block;
    color: var(--color-muted);
    font-weight: 500;
  }

  .descriptor,
  .meta,
  .note {
    display: block;
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-weight: 400;
  }

  .cell {
    display: block;
    min-block-size: 20px;
  }

  .text {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
  }

  td.numeric .cell {
    text-align: end;
    font-variant-numeric: tabular-nums;
  }

  .dash {
    color: var(--color-faint);
  }

  .glyph {
    display: inline-grid;
    place-items: center;
    color: var(--color-faint);
  }

  .glyph.yes {
    color: var(--color-success);
  }

  .aside {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: inherit;
    gap: 4px;
    margin-block-start: 5px;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-smooth);
  }

  td:hover .aside,
  td:focus-within .aside {
    opacity: 1;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-inline-size: 120px;
    padding: 1px 7px 1px 2px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
  }

  .chip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chip:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .chip:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  .editor {
    inline-size: 100%;
    box-sizing: border-box;
    padding: 2px 6px;
    border: 0;
    border-radius: var(--radius-xs);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    outline: 2px solid var(--color-ring);
  }

  .correct {
    padding: 1px 8px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
  }

  .correct:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .correct:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  .general {
    padding: 1px 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-faint);
    font-size: 10px;
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
