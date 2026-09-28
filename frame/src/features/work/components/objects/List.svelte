<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";
  import type { Detail, ListView, ObjectActions } from "../../lib/board/types";
  import MakeTasks from "../board/MakeTasks.svelte";
  import Mark, { hasMark } from "./Mark.svelte";
  import Title from "./Title.svelte";
  import { ArrowUpRight01Icon, Tick02Icon } from "./icons";
  /**
   * Things to do, answer or read, each with where it came from: the app's mark,
   * who and when, the words themselves in a quieter voice, and the way back.
   */
  let {
    object,
    detail,
    actions = {},
  }: { object: ListView; detail: Detail; actions?: ObjectActions } = $props();
  const full = $derived(detail === "full");
  const todo = $derived(object.style === "todo");
  const shown = $derived(detail === "tile" ? [] : object.items);
  const marks = $derived(
    [...new Set(object.items.flatMap((item) => (item.from?.host ? [item.from.host] : [])))].filter(
      (host) => hasMark(host),
    ),
  );
  const open = (url: string) => (event: MouseEvent) => {
    if (!actions.link) return;
    event.preventDefault();
    actions.link(url);
  };
</script>

<section class="list {detail}" aria-label={object.title}>
  {#if object.title}<Title text={object.title} {detail} />{/if}
  <ul class="items">
    {#each shown as item, index (index)}
      <li class:done={item.done} class:high={item.priority === "high"}>
        {#if todo}
          {#if full && actions.check}<button
              type="button"
              class="box nodrag nopan"
              aria-pressed={!!item.done}
              aria-label={item.title}
              onclick={() => actions.check?.(object.id, index, !item.done)}
              >{#if item.done}<Icon icon={Tick02Icon} size={12} />{/if}</button
            >{:else}<span class="box" aria-hidden="true"
              >{#if item.done}<Icon
                  icon={Tick02Icon}
                  size={detail === "full" ? 12 : 24}
                />{/if}</span
            >{/if}
        {:else}<span class="lead" aria-hidden="true"></span>{/if}
        <div class="body">
          <div class="line">
            <h4>
              {#if item.priority === "high"}<span class="flag" title={m.work_list_high()}
                ></span>{/if}{item.title}
            </h4>
            {#if item.due}<span class="due">{item.due}</span>{/if}
          </div>
          {#if full && item.detail}<p class="detail">{item.detail}</p>{/if}
          {#if item.from}
            {@const from = item.from}
            <div class="from">
              {#if from.host}<Mark address={from.host} size={full ? 14 : 28} />{/if}
              <span class="who"
                >{[from.who, full ? from.app : null, full ? from.when : null]
                  .filter(Boolean)
                  .join(" · ")}</span
              >
              {#if from.url && full}<a
                  class="back nodrag"
                  href={from.url}
                  onclick={open(from.url)}
                  title={from.app ? m.work_list_open_in({ app: from.app }) : m.work_env_open()}
                  ><Icon icon={ArrowUpRight01Icon} size={13} /></a
                >{/if}
            </div>
            {#if from.quote && full}<blockquote>{from.quote}</blockquote>{/if}
          {/if}
        </div>
      </li>
    {/each}
  </ul>
  {#if detail === "tile" && marks.length}
    <div class="marks">
      {#each marks as host (host)}<Mark address={host} size={56} />{/each}
    </div>
  {/if}
  {#if todo && full}<footer><MakeTasks id={object.id} /></footer>{/if}
</section>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-inline-size: 640px;
    color: var(--color-text);
  }

  .items {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr);
    column-gap: 14px;
    padding-block: 14px;
    border-block-start: 1px solid var(--color-border);
  }

  li:first-child {
    padding-block-start: 4px;
    border-block-start: 0;
  }

  .box {
    display: grid;
    place-items: center;
    inline-size: 18px;
    block-size: 18px;
    margin-block-start: 1px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-capsule);
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--color-border-strong);
    color: var(--color-surface);
    cursor: default;
  }

  .box:hover {
    box-shadow: inset 0 0 0 1.5px var(--color-muted);
  }

  .done .box {
    background: var(--color-success);
    box-shadow: none;
  }

  .lead {
    inline-size: 6px;
    block-size: 6px;
    margin: 8px auto 0;
    border-radius: var(--radius-capsule);
    background: var(--color-faint);
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-inline-size: 0;
  }

  .line {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 16px;
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

  .flag {
    display: inline-block;
    inline-size: 7px;
    block-size: 7px;
    margin-inline-end: 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-danger);
    vertical-align: 0.12em;
  }

  .due {
    flex: none;
    color: var(--color-muted);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
  }

  .high .due {
    color: var(--color-danger);
  }

  .detail {
    margin: 0;
    color: var(--color-label-secondary);
    font-size: var(--text-body);
    line-height: 19px;
  }

  .from {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .back {
    display: inline-grid;
    place-items: center;
    inline-size: 20px;
    block-size: 20px;
    border-radius: var(--radius-capsule);
    color: var(--color-faint);
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .back:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .back:focus-visible,
  li:hover .back {
    opacity: 1;
  }

  blockquote {
    margin: 2px 0 0;
    padding-inline-start: 12px;
    border-inline-start: 2px solid var(--color-border-strong);
    color: var(--color-label-secondary);
    font-size: var(--text-body);
    line-height: 19px;
    text-wrap: pretty;
  }

  footer {
    padding-inline-start: 34px;
  }

  .overview {
    gap: 24px;
  }

  .overview li {
    grid-template-columns: 40px minmax(0, 1fr);
    column-gap: 24px;
    padding-block: 22px;
    border-block-start-width: 2px;
  }

  .overview .box {
    inline-size: 36px;
    block-size: 36px;
    box-shadow: inset 0 0 0 3px var(--color-border-strong);
  }

  .overview .lead {
    inline-size: 12px;
    block-size: 12px;
    margin-block-start: 14px;
  }

  .overview h4 {
    font-size: var(--text-overview-title);
    line-height: 1.25;
  }

  .overview .flag {
    inline-size: 14px;
    block-size: 14px;
    margin-inline-end: 14px;
  }

  .overview .due,
  .overview .from {
    gap: 12px;
    font-size: var(--text-overview-label);
  }

  .marks {
    display: flex;
    gap: 20px;
  }
</style>
