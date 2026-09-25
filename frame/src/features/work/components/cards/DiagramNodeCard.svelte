<script lang="ts">
  import { getContext, onMount, tick } from "svelte";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import FavIcon from "$shared/ui/FavIcon";
  import Icon from "$shared/ui/Icon";
  import CardFrame from "./CardFrame.svelte";
  import { siteMark } from "./HostGlyph.svelte";
  import {
    CircleIcon,
    CubeIcon,
    Database01Icon,
    FlashIcon,
    Folder01Icon,
    GlobalIcon,
    LaptopIcon,
    LeftToRightListBulletIcon,
    Link04Icon,
    Settings02Icon,
    Shield01Icon,
    SparklesIcon,
  } from "../../lib/icons";
  import { canvasProbe, canvasRename } from "../../lib/canvas-context";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected }: { item: CanvasItem; selected: boolean } = $props();
  /** A closed table: every part kind has its glyph, and an unknown one a dot. */
  const GLYPHS: Record<string, IconSvgElement> = {
    client: LaptopIcon,
    edge: GlobalIcon,
    gateway: Shield01Icon,
    service: CubeIcon,
    worker: Settings02Icon,
    model: SparklesIcon,
    store: Database01Icon,
    queue: LeftToRightListBulletIcon,
    cache: FlashIcon,
    storage: Folder01Icon,
    external: Link04Icon,
    other: CircleIcon,
  };
  const rename = getContext<
    { can: (id: string) => boolean; rename: (id: string, name: string) => void } | undefined
  >(canvasRename);
  const probe = getContext<((origin: string) => void) | undefined>(canvasProbe);
  const vendor = $derived(item.diagram?.vendor?.trim().toLowerCase() ?? "");
  const origin = $derived(vendor ? `https://${vendor}` : "");
  const mark = $derived(origin ? siteMark(origin) : null);
  const glyph = $derived(GLYPHS[item.diagram?.kind ?? "other"] ?? CircleIcon);
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
  <div class="part" data-kind={item.diagram?.kind ?? "other"}>
    <span class="mark" class:vendor={!!mark} aria-hidden="true">
      {#if mark}<FavIcon image={mark.image} tone={mark.tone} size={24} />{:else}<Icon
          icon={glyph}
          size={14}
        />{/if}
    </span>
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
        />{:else}<strong class="name" title={item.title} ondblclick={edit}>{item.title}</strong
        >{/if}
      <span class="caption">{item.diagram?.note || item.kind}</span>
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
  }

  .mark {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 24px;
    block-size: 24px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    color: var(--color-label-secondary);
  }

  .mark.vendor {
    background: transparent;
  }

  .words {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1;
    min-inline-size: 0;
  }

  .name,
  .caption {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 17px;
    letter-spacing: -0.005em;
  }

  .caption {
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
