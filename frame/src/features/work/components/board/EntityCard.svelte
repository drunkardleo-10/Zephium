<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import { mediaUrl } from "$domain/resources";
  import type { EvidenceReference } from "$shared/ui/data/Artifact";
  import HostGlyph from "../cards/HostGlyph.svelte";
  import Chips from "./Chips.svelte";
  import Popover from "./Popover.svelte";
  import { Tick02Icon } from "../../lib/icons";
  import type { BoardActions } from "../../lib/canvas-context";
  import type { Entity } from "../../lib/board/types";
  import * as m from "$shared/i18n/messages";
  let {
    entity,
    sources,
    actions,
    wide = false,
  }: {
    entity: Entity;
    sources: Readonly<Record<string, EvidenceReference>>;
    actions?: BoardActions;
    /** The entity stands alone: its picture takes the block's width. */
    wide?: boolean;
  } = $props();
  const picture = $derived(
    entity.image ? mediaUrl(entity.image.profile, entity.image.digest) : null,
  );
  let failed = $state(false);
  const host = $derived.by(() => {
    const first = entity.sources?.[0];
    const address = entity.homepage ?? (first ? sources[first]?.url : undefined) ?? "";
    try {
      return address ? new URL(address).host.replace(/^www\./u, "") : "";
    } catch {
      return "";
    }
  });
  const typed = $derived(entity.facts.filter((fact) => fact.label));
  const said = $derived(entity.facts.filter((fact) => !fact.label));
  let open = $state(false);
</script>

<div class="entity" class:wide class:chosen={entity.chosen}>
  <button
    type="button"
    class="face nodrag nopan"
    aria-expanded={open}
    aria-label={entity.name}
    onclick={() => (open = !open)}
  >
    {#if picture && !failed}<span class="picture"
        ><img
          src={picture}
          alt=""
          loading="lazy"
          decoding="async"
          draggable="false"
          width={wide ? 320 : 216}
          height={wide ? 200 : 136}
          onerror={() => (failed = true)}
        /></span
      >{:else}<span class="logo"
        ><HostGlyph url={entity.homepage ?? ""} {host} size={20} initial={false} /></span
      >{/if}
    <span class="words">
      <strong class="name">{entity.name}</strong>
      {#if entity.price || entity.time}<span class="money"
          >{#if entity.price}<span class="price">{entity.price}</span>{/if}{#if entity.time}<span
              class="time">{entity.time}</span
            >{/if}</span
        >{:else if entity.descriptor}<span class="descriptor">{entity.descriptor}</span>{/if}
      {#if typed.length}<span class="facts">
          {#each typed.slice(0, wide ? 4 : 3) as fact (fact.label)}<span class="fact"
              ><span class="label">{fact.label}</span><span class="value">{fact.value}</span></span
            >{/each}
        </span>{/if}
      {#if said[0]}<span class="said">{said[0].value}</span>{/if}
      {#if host}<span class="host"><HostGlyph {host} size={12} initial={false} />{host}</span>{/if}
    </span>
  </button>
  {#if actions}<button
      type="button"
      class="choose nodrag nopan"
      class:on={entity.chosen}
      aria-pressed={!!entity.chosen}
      title={entity.chosen ? m.work_env_unchoose() : m.work_env_choose()}
      aria-label={entity.chosen ? m.work_env_unchoose() : m.work_env_choose()}
      onclick={() => entity.element && actions.choose(entity.element, !entity.chosen)}
      ><Icon icon={Tick02Icon} size={13} /></button
    >{/if}
  {#if open}
    <Popover label={entity.name} side={wide ? "right" : "below"} onclose={() => (open = false)}>
      <p class="pop-name">{entity.name}</p>
      {#if entity.descriptor}<p class="pop-descriptor">{entity.descriptor}</p>{/if}
      {#if entity.price || entity.time}<p class="pop-money">
          {[entity.price, entity.time].filter(Boolean).join(" · ")}
        </p>{/if}
      {#if typed.length}<dl class="pop-facts">
          {#each typed as fact (fact.label)}<div>
              <dt>{fact.label}</dt>
              <dd>{fact.value}</dd>
            </div>{/each}
        </dl>{/if}
      {#if said.length}<ul class="pop-said">
          {#each said as fact (fact.value)}<li>
              {fact.value}{#if fact.sources?.length}<Chips
                  keys={fact.sources}
                  {sources}
                  onopen={actions?.evidence}
                />{/if}
            </li>{/each}
        </ul>{/if}
      <div class="pop-actions">
        {#if actions && entity.element}<button
            type="button"
            class="primary"
            onclick={() => {
              open = false;
              actions.entity(entity.element!);
            }}>{m.work_board_details()}</button
          >{/if}
        {#if actions}<button
            type="button"
            onclick={() => {
              open = false;
              actions.ask(entity.name);
            }}>{m.work_board_ask()}</button
          >{/if}
        {#if actions && entity.homepage}<button
            type="button"
            onclick={() => {
              open = false;
              actions.page(entity.homepage!);
            }}>{m.work_board_visit({ host })}</button
          >{/if}
      </div>
    </Popover>
  {/if}
</div>

<style>
  .entity {
    position: relative;
    min-inline-size: 0;
  }

  .face {
    display: flex;
    flex-direction: column;
    gap: 10px;
    inline-size: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
  }

  .face:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 4px;
    border-radius: var(--radius-row);
  }

  .picture {
    display: grid;
    place-items: center;
    overflow: hidden;
    inline-size: 100%;
    aspect-ratio: 16 / 10;
    border-radius: var(--radius-row);
    background: var(--color-fill);
    box-shadow: inset 0 0 0 1px var(--color-border);
  }

  /* Without a picture the site's mark stands for it, as an app's icon would. */
  .logo {
    display: grid;
    place-items: center;
    inline-size: 36px;
    block-size: 36px;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
    box-shadow: inset 0 0 0 1px var(--color-border);
  }

  .chosen .picture,
  .chosen .logo {
    box-shadow: 0 0 0 2px var(--color-lit);
  }

  img {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
    transition: transform var(--motion-base) var(--ease-smooth);
  }

  .face:hover img {
    transform: scale(1.02);
  }

  .words {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-inline-size: 0;
  }

  .name {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 17px;
    letter-spacing: -0.005em;
  }

  .money {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-inline-size: 0;
    font-variant-numeric: tabular-nums;
  }

  .price {
    font-size: var(--text-body);
    font-weight: 500;
  }

  .time,
  .descriptor {
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-label);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .facts {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-block-start: 2px;
  }

  .fact {
    display: flex;
    gap: 6px;
    min-inline-size: 0;
    font-size: var(--text-label);
    line-height: 16px;
  }

  .label {
    flex: none;
    color: var(--color-faint);
  }

  .value {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-label-secondary);
  }

  .said {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    margin-block-start: 4px;
    color: var(--color-label-secondary);
    font-size: var(--text-label);
    line-height: 16px;
  }

  .host {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-block-start: 4px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .choose {
    position: absolute;
    inset-block-start: 8px;
    inset-inline-end: 8px;
    display: grid;
    place-items: center;
    inline-size: 24px;
    block-size: 24px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-capsule);
    background: color-mix(in srgb, var(--color-float) 80%, transparent);
    box-shadow: var(--shadow-raised);
    color: var(--color-faint);
    cursor: default;
    opacity: 0;
    transition:
      opacity var(--motion-fast) var(--ease-out),
      background-color var(--motion-fast) var(--ease-out);
  }

  .entity:hover .choose,
  .entity .choose:focus-visible,
  .entity .choose.on {
    opacity: 1;
  }

  .entity .choose.on {
    background: var(--color-lit);
    color: var(--color-on-lit);
  }

  .pop-name {
    margin: 0;
    font-weight: 600;
    line-height: 18px;
  }

  .pop-descriptor,
  .pop-money {
    margin: 4px 0 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
  }

  .pop-money {
    color: var(--color-text);
    font-size: var(--text-body);
  }

  .pop-facts {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 12px 0 0;
    font-size: var(--text-label);
  }

  .pop-facts div {
    display: flex;
    gap: 8px;
  }

  .pop-facts dt {
    flex: none;
    min-inline-size: 72px;
    color: var(--color-faint);
  }

  .pop-facts dd {
    margin: 0;
    color: var(--color-label-secondary);
  }

  .pop-said {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 12px 0 0;
    padding-inline-start: 16px;
    color: var(--color-label-secondary);
    font-size: var(--text-label);
    line-height: 17px;
  }

  .pop-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-block-start: 14px;
  }

  .pop-actions button {
    block-size: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: var(--color-control);
    color: var(--color-on-control);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .pop-actions button:hover {
    background: var(--color-control-hover);
  }

  .pop-actions .primary {
    background: var(--color-lit);
    color: var(--color-on-lit);
  }

  .pop-actions .primary:hover {
    background: var(--color-lit-hover);
  }
</style>
