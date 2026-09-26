<script lang="ts">
  import { getContext, onMount, tick } from "svelte";
  import FavIcon from "$shared/ui/FavIcon";
  import Icon from "$shared/ui/Icon";
  import CardFrame from "./CardFrame.svelte";
  import { siteMark } from "./HostGlyph.svelte";
  import { canvasDiagram, canvasProbe, canvasRename } from "../../lib/canvas-context";
  import { vendorHost } from "../../lib/vendors";
  import { diagramGlyph } from "../../lib/diagram-glyphs";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  const rename = getContext<
    { can: (id: string) => boolean; rename: (id: string, name: string) => void } | undefined
  >(canvasRename);
  const probe = getContext<((origin: string) => void) | undefined>(canvasProbe);
  // The stated vendor, else a well-known product the part's name or note names.
  const vendor = $derived(vendorHost(item.diagram?.vendor, item.title, item.diagram?.note) ?? "");
  const origin = $derived(vendor ? `https://${vendor}` : "");
  const mark = $derived(origin ? siteMark(origin) : null);
  const glyph = $derived(diagramGlyph(item.diagram?.kind));
  /** Another part is being looked at and no flow joins this one to it. */
  const dim = getContext<((id: string) => boolean) | undefined>(canvasDiagram);
  const dimmed = $derived(!!dim?.(item.id));
  // Until the vendor's icon is held, the kind's glyph stands; native is asked once.
  onMount(() => {
    if (origin && !mark) probe?.(origin);
  });
  let editing = $state(false);
  let draft = $state("");
  let input = $state<HTMLInputElement>();
  async function edit(event: MouseEvent) {
    if (!rename?.can(item.id)) return;
    event.stopPropagation();
    draft = item.title;
    editing = true;
    await tick();
    input?.focus();
    input?.select();
  }
  function commit() {
    if (!editing) return;
    editing = false;
    const name = draft.trim();
    if (name && name !== item.title) rename?.rename(item.id, name);
  }
</script>

<!-- One part of a system: what it is at a glance, its name, what it does. -->
<CardFrame id={item.id} {selected} plain>
  <div class="part" class:dimmed data-kind={item.diagram?.kind ?? "other"}>
    {#if mark}<span class="mark" aria-hidden="true"
        ><FavIcon image={mark.image} tone={mark.tone} size={24} /></span
      >{/if}
    <span class="words">
      {#if editing}<input
          bind:this={input}
          bind:value={draft}
          class="name-input nodrag nopan"
          aria-label={m.work_diagram_rename()}
          maxlength="256"
          onkeydown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              commit();
            } else if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              editing = false;
            }
          }}
          onblur={commit}
        />{:else}<strong class="name" title={item.title} ondblclick={edit}
          >{#if !mark}<span class="glyph" aria-hidden="true"><Icon icon={glyph} size={12} /></span
            >{/if}{item.title}</strong
        >{/if}
      <span class="caption" title={item.diagram?.note || undefined}
        >{item.diagram?.note || item.kind}</span
      >
    </span>
  </div>
</CardFrame>

<style>
  .part {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    block-size: 100%;
    padding: 0 12px;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  /* Another part is looked at: this one steps back. */
  .part.dimmed {
    opacity: 0.6;
  }

  .mark {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 24px;
    block-size: 24px;
  }

  /* A part with no vendor says what it is by a small glyph before its name. */
  .glyph {
    display: inline-flex;
    margin-inline-end: 5px;
    color: var(--color-label-secondary);
    vertical-align: -1px;
  }

  .words {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1;
    min-inline-size: 0;
  }

  /* The name on one line, two when it wraps; what the part does on up to two more. */
  .name {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    overflow-wrap: anywhere;
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 17px;
    letter-spacing: -0.005em;
  }

  .caption {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    overflow-wrap: anywhere;
    color: var(--color-muted);
    font-size: var(--text-caption);
    line-height: 13px;
  }

  .name-input {
    min-inline-size: 0;
    margin: -2px -6px;
    padding: 1px 5px;
    border: 1px solid var(--color-lit);
    border-radius: var(--radius-inset);
    background: var(--color-surface);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 17px;
    outline: none;
  }
</style>
