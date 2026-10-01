<script lang="ts">
  import CardFrame from "./CardFrame.svelte";
  import { Note01Icon } from "../../lib/icons";
  import type { CanvasItem } from "../../lib/canvas-model";
  import { NOTE_HUES, chooseNoteHue, noteHue } from "../../lib/note-hue.svelte";
  import * as m from "$shared/i18n/messages";
  /** One of the person's notes on the canvas: a sheet in its own soft hue, its title and first words. */
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const updated = $derived.by(() => {
    // Notes say when as milliseconds since the epoch, in decimal.
    const date = new Date(/^\d+$/u.test(item.detail) ? Number(item.detail) : item.detail);
    return Number.isNaN(date.getTime())
      ? ""
      : date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  });
  const hue = $derived(item.note ? noteHue(item.note.id) : "lemon");
  const HUE_NAME = {
    lemon: m.work_note_hue_lemon,
    sky: m.work_note_hue_sky,
    mint: m.work_note_hue_mint,
    peach: m.work_note_hue_peach,
    lilac: m.work_note_hue_lilac,
    rose: m.work_note_hue_rose,
  } as const;
</script>

{#if item.note && !item.unavailable}
  <article
    class="note work-drag-handle"
    class:selected
    data-card-id={item.id}
    style:--note-wash="var(--color-soft-{hue}-wash)"
    style:--note-edge="var(--color-soft-{hue}-edge)"
    style:--note-ink="var(--color-soft-{hue}-ink)"
  >
    <h3>{item.title}</h3>
    {#if item.note.preview}<p>{item.note.preview}</p>{/if}
    <footer>
      <span class="when">{updated}</span>
      <span class="hues" role="radiogroup" aria-label={m.work_note_colour()}>
        {#each NOTE_HUES as each (each)}<button
            type="button"
            role="radio"
            class="hue nodrag nopan"
            aria-checked={each === hue}
            aria-label={HUE_NAME[each]()}
            style:--dot="var(--color-soft-{each})"
            onclick={(event) => {
              event.stopPropagation();
              chooseNoteHue(item.note!.id, each);
            }}
          ></button>{/each}
      </span>
    </footer>
  </article>
{:else}
  <CardFrame
    id={item.id}
    kind={item.kind}
    title={item.title}
    icon={Note01Icon}
    {selected}
    unavailable={item.unavailable}
  >
    {#snippet footer()}<span>{item.status}</span><span>{updated}</span>{/snippet}
  </CardFrame>
{/if}

<style>
  .note {
    display: flex;
    box-sizing: border-box;
    flex-direction: column;
    gap: 8px;
    inline-size: 100%;
    block-size: 100%;
    padding: 16px 18px 12px;
    overflow: hidden;
    border-radius: var(--radius-card);
    background: var(--note-wash);
    box-shadow: inset 0 0 0 1px var(--note-edge);
    color: var(--color-text);
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  .note.selected {
    box-shadow:
      inset 0 0 0 1px var(--note-edge),
      0 0 0 1.5px var(--color-ring);
  }

  h3 {
    display: -webkit-box;
    margin: 0;
    overflow: hidden;
    font-size: var(--text-title);
    font-weight: 650;
    line-height: 1.25;
    letter-spacing: -0.01em;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  p {
    display: -webkit-box;
    margin: 0;
    overflow: hidden;
    color: var(--color-label-secondary);
    font-size: var(--text-reading);
    line-height: 1.45;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 4;
    line-clamp: 4;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-block-start: auto;
    min-block-size: 20px;
  }

  .when {
    color: var(--note-ink);
    font-size: var(--text-caption);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  .hues {
    display: flex;
    gap: 6px;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .hues:focus-within,
  .note:hover .hues {
    opacity: 1;
  }

  .hue {
    inline-size: 12px;
    block-size: 12px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--dot);
    box-shadow: inset 0 0 0 1px var(--note-edge);
    cursor: default;
  }

  .hue[aria-checked="true"] {
    box-shadow:
      0 0 0 2px var(--note-wash),
      0 0 0 3px var(--dot);
  }

  .hue:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>
