<script lang="ts">
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import Icon from "$shared/ui/Icon";
  import HostGlyph from "./cards/HostGlyph.svelte";
  import { ArrowDown01Icon } from "../lib/icons";
  import * as m from "$shared/i18n/messages";
  let {
    references,
    onpick,
  }: {
    references: readonly EvidenceReference[];
    /** A page opens in the pane, a file in its view, retained text where it is. */
    onpick: (reference: EvidenceReference) => void;
  } = $props();
  const GLYPHS = 6;
  let open = $state(false);
  const where = (reference: EvidenceReference) =>
    reference.file ? reference.label : reference.origin || reference.label;
  /** One glyph per site or file, in the order the result cites them. */
  const glyphs = $derived([
    ...new Map(
      references.map((reference) => [
        reference.file ? `file:${reference.file.path}` : where(reference),
        reference,
      ]),
    ).values(),
  ]);
</script>

{#if references.length}
  <section class="rail" aria-label={m.work_sources()}>
    <button type="button" class="summary" aria-expanded={open} onclick={() => (open = !open)}>
      <span class="glyphs" aria-hidden="true">
        {#each glyphs.slice(0, GLYPHS) as reference (reference.key)}<span class="glyph"
            ><HostGlyph
              host={where(reference)}
              url={reference.url}
              file={!!reference.file}
              size={16}
            /></span
          >{/each}
      </span>
      <span class="label"
        >{references.length === 1
          ? m.work_lift_based_on_one()
          : m.work_lift_based_on({ count: references.length })}</span
      >
      <span class="chevron" class:open aria-hidden="true"
        ><Icon icon={ArrowDown01Icon} size={14} /></span
      >
    </button>
    {#if open}<ul>
        {#each references as reference (reference.key)}<li>
            <button
              type="button"
              title={reference.url ?? reference.label}
              onclick={() => onpick(reference)}
            >
              <HostGlyph
                host={where(reference)}
                url={reference.url}
                file={!!reference.file}
                size={16}
              />
              <span class="host">{where(reference)}</span>
              {#if reference.label && reference.label !== where(reference)}<span class="title"
                  >{reference.label}</span
                >{/if}
            </button>
          </li>{/each}
      </ul>{/if}
  </section>
{/if}

<style>
  .rail {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-block-start: 12px;
    border-block-start: 1px solid var(--color-border);
  }

  button {
    display: flex;
    align-items: center;
    gap: 8px;
    inline-size: 100%;
    box-sizing: border-box;
    padding: 6px 8px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  button:hover {
    background: var(--row-hover);
  }

  button:active {
    background: var(--row-pressed);
  }

  button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .summary {
    color: var(--color-muted);
  }

  .glyphs {
    display: inline-flex;
    flex: none;
  }

  .glyph {
    display: inline-grid;
    margin-inline-start: -4px;
    border-radius: calc(var(--radius-inset) / 2);
    background: var(--color-surface);
    box-shadow: 0 0 0 1.5px var(--color-surface);
  }

  .glyph:first-child {
    margin-inline-start: 0;
  }

  .label {
    flex: 1;
    min-inline-size: 0;
  }

  .chevron {
    display: inline-grid;
    transition: rotate var(--motion-base) var(--ease-emphasized);
  }

  .chevron.open {
    rotate: 180deg;
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .host {
    flex: none;
    max-inline-size: 40%;
    font-weight: 550;
  }

  .title {
    min-inline-size: 0;
    color: var(--color-muted);
  }

  .host,
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
