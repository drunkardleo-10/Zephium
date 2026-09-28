<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import type { Detail, ObjectActions, PlanView } from "../../lib/board/types";
  import MakeTasks from "../board/MakeTasks.svelte";
  import Mark, { hasMark } from "./Mark.svelte";
  import Title from "./Title.svelte";
  import {
    AirplaneTakeOff01Icon,
    BedDoubleIcon,
    Calendar03Icon,
    CheckmarkCircle02Icon,
    Flag02Icon,
    Note01Icon,
    Tick02Icon,
  } from "./icons";
  /** Steps in time along one spine: when, what, what it costs, and the pick it stands on. */
  let {
    object,
    detail,
    actions = {},
  }: { object: PlanView; detail: Detail; actions?: ObjectActions } = $props();
  const GLYPH = {
    travel: AirplaneTakeOff01Icon,
    stay: BedDoubleIcon,
    event: Calendar03Icon,
    task: CheckmarkCircle02Icon,
    milestone: Flag02Icon,
    note: Note01Icon,
  } as const;
  const full = $derived(detail === "full");
  const priced = $derived(full && object.steps.some((step) => step.cost));
  /** A step names its day only where the day changes. */
  const dated = $derived(
    object.steps.map((step, index) =>
      index > 0 && step.when === object.steps[index - 1]?.when ? "" : (step.when ?? ""),
    ),
  );
  const glyph = (size: number) => (detail === "full" ? size : size * 2);
</script>

<section class="plan {detail}" class:priced aria-label={object.title}>
  {#if object.title}<Title text={object.title} {detail} />{/if}
  <ol class="steps">
    {#each object.steps as step, index (index)}
      <li class="step {step.kind}" class:done={step.done} class:first={!!dated[index]}>
        <span class="when">{detail === "tile" ? "" : dated[index]}</span>
        <span class="node">
          {#if object.checkable && full && actions.check}<button
              type="button"
              class="glyph check nodrag nopan"
              aria-pressed={!!step.done}
              aria-label={step.title}
              onclick={() => actions.check?.(object.id, index, !step.done)}
              ><Icon icon={step.done ? Tick02Icon : GLYPH[step.kind]} size={glyph(14)} /></button
            >{:else}<span class="glyph"
              ><Icon icon={step.done ? Tick02Icon : GLYPH[step.kind]} size={glyph(14)} /></span
            >{/if}
        </span>
        {#if detail !== "tile"}
          <div class="what">
            <div class="head">
              <h4>{step.title}</h4>
              {#if step.pick && full}
                {#if step.pick.picture}<img
                    class="thumb"
                    src={step.pick.picture.src}
                    alt={step.pick.name}
                    title={step.pick.name}
                    decoding="async"
                    loading="lazy"
                    width="40"
                    height="30"
                  />{:else if step.pick.logo && hasMark(step.pick.logo)}<span
                    class="logo"
                    title={step.pick.name}><Mark address={step.pick.logo} size={16} /></span
                  >{/if}
              {/if}
            </div>
            {#if full && (step.detail || step.place)}
              <p class="detail">
                {#if step.place}<span class="place">{step.place}{step.detail ? " · " : ""}</span
                  >{/if}{#if step.detail}{step.detail}{/if}
              </p>
            {/if}
          </div>
          {#if priced}<span class="cost">{step.cost ?? ""}</span>{/if}
        {/if}
      </li>
    {/each}
  </ol>
  {#if object.total && detail !== "tile"}
    <p class="total">
      <span class="label">{object.total.label}</span><span class="value">{object.total.value}</span>
    </p>
  {/if}
  {#if object.checkable && full}<footer><MakeTasks id={object.id} /></footer>{/if}
</section>

<style>
  .plan {
    --when: 92px;
    --node: 28px;

    display: flex;
    flex-direction: column;
    gap: 16px;
    max-inline-size: 760px;
    color: var(--color-text);
  }

  .steps {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .step {
    position: relative;
    display: grid;
    grid-template-columns: var(--when) var(--node) minmax(0, 1fr);
    column-gap: 14px;
    padding-block-end: 18px;
  }

  .priced .step {
    grid-template-columns: var(--when) var(--node) minmax(0, 1fr) auto;
  }

  /* The spine: one hairline from each node down to the next. */
  .step:not(:last-child)::before {
    position: absolute;
    inset-block: calc(var(--node) + 4px) 4px;
    inset-inline-start: calc(var(--when) + 14px + var(--node) / 2 - 0.5px);
    inline-size: 1px;
    background: var(--color-border-strong);
    content: "";
  }

  .step.first:not(:first-child) {
    padding-block-start: 10px;
  }

  .step.first:not(:first-child)::after {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: calc(var(--when) + 14px + var(--node) / 2 - 0.5px);
    inline-size: 1px;
    block-size: 10px;
    background: var(--color-border-strong);
    content: "";
  }

  .when {
    padding-block-start: 7px;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .node {
    display: flex;
    justify-content: center;
  }

  .glyph {
    display: grid;
    place-items: center;
    inline-size: var(--node);
    block-size: var(--node);
    padding: 0;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-active);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-label-secondary);
    cursor: default;
  }

  .milestone .glyph {
    background: var(--color-lit);
    color: var(--color-on-lit);
  }

  .done .glyph {
    background: var(--color-success);
    box-shadow: none;
    color: var(--color-surface);
  }

  .check:hover {
    box-shadow: inset 0 0 0 1.5px var(--color-border-strong);
  }

  .check:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .what {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-inline-size: 0;
    padding-block-start: 4px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  h4 {
    margin: 0;
    font-size: var(--text-page-title);
    font-weight: 600;
    line-height: 20px;
    text-wrap: pretty;
  }

  .done h4 {
    color: var(--color-muted);
  }

  .thumb {
    flex: none;
    inline-size: 40px;
    block-size: 30px;
    border-radius: var(--radius-inset);
    object-fit: cover;
    box-shadow: 0 0 0 1px var(--color-border);
  }

  .logo {
    display: inline-flex;
  }

  .detail {
    max-inline-size: 60ch;
    margin: 0;
    color: var(--color-label-secondary);
    font-size: var(--text-body);
    line-height: 19px;
    text-wrap: pretty;
  }

  .place {
    color: var(--color-muted);
  }

  .cost {
    padding-block-start: 5px;
    color: var(--color-label-secondary);
    font-size: var(--text-body);
    font-variant-numeric: tabular-nums;
    text-align: end;
    white-space: nowrap;
  }

  .total {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 16px;
    margin: 0;
    margin-inline-start: calc(var(--when) + var(--node) + 28px);
    padding-block-start: 14px;
    border-block-start: 1px solid var(--color-border-strong);
  }

  .total .label {
    color: var(--color-muted);
    font-size: var(--text-label);
    font-weight: 500;
  }

  .total .value {
    font-size: var(--text-title);
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.015em;
  }

  footer {
    margin-inline-start: calc(var(--when) + var(--node) + 28px);
  }

  .overview {
    --when: 200px;
    --node: 56px;

    gap: 28px;
  }

  .overview .step {
    column-gap: 24px;
    padding-block-end: 28px;
  }

  .overview .step:not(:last-child)::before,
  .overview .step.first:not(:first-child)::after {
    inset-inline-start: calc(var(--when) + 24px + var(--node) / 2 - 1px);
    inline-size: 2px;
  }

  .overview .when,
  .overview .total .label {
    padding-block-start: 14px;
    font-size: var(--text-overview-label);
  }

  .overview h4 {
    padding-block-start: 8px;
    font-size: var(--text-overview-title);
    line-height: 1.25;
  }

  .overview .total {
    margin-inline-start: calc(var(--when) + var(--node) + 48px);
    border-block-start-width: 2px;
  }

  .overview .total .value {
    font-size: var(--text-overview-figure);
  }

  .tile {
    --when: 0px;
    --node: 44px;

    gap: 28px;
  }

  .tile .step {
    grid-template-columns: var(--node);
    column-gap: 0;
    padding-block-end: 20px;
  }

  .tile .when {
    display: none;
  }

  .tile .step:not(:last-child)::before,
  .tile .step.first:not(:first-child)::after {
    inset-inline-start: calc(var(--node) / 2 - 1.5px);
    inline-size: 3px;
  }
</style>
