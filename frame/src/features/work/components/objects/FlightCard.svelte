<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";
  import type { ObjectActions, PickView } from "../../lib/board/types";
  import Mark, { hasMark } from "./Mark.svelte";
  import YesNo from "./YesNo.svelte";
  import { Tick02Icon } from "./icons";
  import Star from "./Star.svelte";
  /** A flight as its route: times over the airports, the stops as dots on the way. */
  let {
    pick,
    actions = {},
  }: {
    pick: PickView & { route: NonNullable<PickView["route"]> };
    actions?: ObjectActions;
  } = $props();
  const route = $derived(pick.route);
  const carrier = $derived(route.carrierHost ?? pick.logo);
  const stops = $derived(
    route.stops === 0
      ? m.work_flight_direct()
      : route.via?.length
        ? m.work_flight_via({ places: route.via.join(", ") })
        : m.work_flight_stops({ count: route.stops }),
  );
</script>

<article class="flight" class:recommended={pick.recommended} class:chosen={pick.chosen}>
  <header>
    {#if carrier && hasMark(carrier)}<Mark address={carrier} size={20} />{/if}
    <span class="carrier">{route.carrier ?? pick.name}</span>
    {#if pick.recommended}<span class="flag"><Star size={11} />{m.work_pick_top()}</span>{/if}
    {#if pick.price}<span class="price">{pick.price.display}</span>{/if}
  </header>
  {#if pick.when}<p class="when">{pick.when}</p>{/if}
  <div class="route">
    <div class="end">
      <span class="time">{route.depart ?? route.from}</span>
      {#if route.depart}<span class="place">{route.from}</span>{/if}
    </div>
    <div class="way">
      {#if route.duration}<span class="duration">{route.duration}</span>{/if}
      <span class="line" aria-hidden="true">
        {#each Array.from({ length: route.stops }, (_, index) => index) as index (index)}<span
            class="stop"
            style:inset-inline-start={`${((index + 1) / (route.stops + 1)) * 100}%`}
          ></span>{/each}
      </span>
      <span class="stops" class:direct={route.stops === 0}>{stops}</span>
    </div>
    <div class="end arrive">
      <span class="time">{route.arrive ?? route.to}</span>
      {#if route.arrive}<span class="place">{route.to}</span>{/if}
    </div>
  </div>
  {#if pick.facts.length}
    <ul class="facts">
      {#each pick.facts as fact (fact.label)}
        <li>
          {#if fact.kind === "yes" || fact.kind === "no" || fact.kind === "partial"}<YesNo
              value={fact.kind}
              size={14}
            />{/if}{#if !/^(yes|no)$/iu.test(fact.value.trim())}<span
              class:quiet={fact.kind === "no"}>{fact.value}</span
            >{/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if pick.why}<p class="why">{pick.why}</p>{/if}
  {#if actions.choose || actions.ask}
    <div class="actions">
      {#if actions.choose && pick.element}<button
          type="button"
          class="nodrag nopan"
          onclick={() => actions.choose?.(pick.element!, !pick.chosen)}
          >{#if pick.chosen}<Icon
              icon={Tick02Icon}
              size={13}
            />{m.work_env_decided()}{:else}{m.work_env_choose()}{/if}</button
        >{/if}
      {#if actions.ask}<button
          type="button"
          class="nodrag nopan"
          onclick={() => actions.ask?.(pick.name)}>{m.work_env_ask()}</button
        >{/if}
    </div>
  {/if}
</article>

<style>
  .flight {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 14px;
    box-sizing: border-box;
    padding: 16px 18px 18px;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
    color: var(--color-text);
  }

  .flight.recommended {
    box-shadow:
      0 0 0 1.5px var(--color-border-strong),
      var(--shadow-raised);
  }

  .flight.chosen {
    box-shadow:
      0 0 0 2px var(--color-success),
      var(--shadow-raised);
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
  }

  .carrier {
    min-inline-size: 0;
    font-size: var(--text-body);
    font-weight: 600;
  }

  .flag {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 600;
  }

  .price {
    margin-inline-start: auto;
    font-size: calc(var(--text-page-title) + 3px);
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.01em;
  }

  .when {
    margin: -8px 0 0;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .route {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 14px;
  }

  .end {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .arrive {
    align-items: flex-end;
  }

  .time {
    font-size: calc(var(--text-title) - 2px);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.015em;
    line-height: 1.1;
  }

  .place {
    color: var(--color-muted);
    font-size: var(--text-label);
    font-weight: 550;
    letter-spacing: 0.04em;
  }

  .way {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 5px;
  }

  .duration,
  .stops {
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }

  .stops.direct {
    color: var(--color-success);
  }

  .line {
    position: relative;
    inline-size: 100%;
    block-size: 1.5px;
    border-radius: var(--radius-capsule);
    background: var(--color-border-strong);
  }

  .line::before,
  .line::after {
    position: absolute;
    inset-block-start: -2.25px;
    inline-size: 6px;
    block-size: 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-muted);
    content: "";
  }

  .line::before {
    inset-inline-start: -3px;
  }

  .line::after {
    inset-inline-end: -3px;
  }

  .stop {
    position: absolute;
    inset-block-start: -3.25px;
    inline-size: 8px;
    block-size: 8px;
    margin-inline-start: -4px;
    border-radius: var(--radius-capsule);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1.5px var(--color-muted);
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 14px;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--text-label);
  }

  .facts li {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .quiet {
    color: var(--color-muted);
  }

  .why {
    margin: 0;
    color: var(--color-label-secondary);
    font-size: var(--text-label);
    line-height: 17px;
  }

  .actions {
    position: absolute;
    inset-block-end: 12px;
    inset-inline-end: 12px;
    display: flex;
    gap: 4px;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .flight:hover .actions,
  .flight:focus-within .actions {
    opacity: 1;
  }

  .actions button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    block-size: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-float);
    box-shadow: var(--shadow-float);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 550;
    cursor: default;
  }

  .actions button:hover {
    background: var(--color-lit);
    color: var(--color-on-lit);
  }
</style>
