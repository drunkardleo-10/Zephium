<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import type { SurfaceRow } from "../lib/search-surface.svelte";
  import type { ResultSection } from "../lib/search-model";
  import { groupRows } from "../lib/search-model";
  import ResultRow from "./ResultRow.svelte";

  let {
    rows,
    selected,
    query,
    listId,
    busy,
    onhover,
    onrun,
    ref = $bindable(),
  }: {
    rows: SurfaceRow[];
    selected: string | null;
    /** The query the rows answer, not the live text. See `answered`. */
    query: string;
    listId: string;
    busy: boolean;
    onhover: (id: string) => void;
    onrun: (row: SurfaceRow) => void;
    ref?: HTMLElement;
  } = $props();

  const sectionLabels: Record<ResultSection, () => string> = {
    search: m.panel_search_mode,
    tabs: m.search_tabs,
    history: m.search_history,
    notes: m.search_notes,
    commands: m.search_commands,
  };
  let groups = $derived(groupRows(rows));
  // A heading over the only group on screen names something the user can
  // already see. Headings earn their line once the list mixes sources.
  let headed = $derived(groups.length > 1);
</script>

<div id={listId} bind:this={ref} role="listbox" aria-label={m.panel_results()} aria-busy={busy}>
  {#each groups as group (group.section)}
    <div
      role="group"
      aria-label={sectionLabels[group.section]()}
      aria-labelledby={headed ? `${listId}-${group.section}` : undefined}
    >
      {#if headed}<div id={`${listId}-${group.section}`} class="heading" role="presentation">
          {sectionLabels[group.section]()}
        </div>{/if}
      {#each group.rows as row (row.id)}
        {@const index = rows.indexOf(row)}
        <ResultRow
          id={`${listId}-option-${index}`}
          title={row.title}
          result={row.result}
          icon={row.icon}
          {index}
          setsize={rows.length}
          selected={selected === row.id}
          {query}
          onhover={() => onhover(row.id)}
          onrun={() => onrun(row)}
        />
      {/each}
    </div>
  {/each}
</div>

<style>
  .heading {
    font-size: 11px;
    font-weight: 550;
    letter-spacing: 0.02em;
    color: var(--color-faint);
    padding-inline: 10px;
    margin-block: 10px 4px;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
  }

  div[role="group"]:first-child .heading {
    margin-block-start: 2px;
  }
</style>
