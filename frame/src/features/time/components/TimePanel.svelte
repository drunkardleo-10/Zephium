<!--
  Time in the sidebar: the day or week at a glance, focus, and where the time
  went. Everything deeper lives on the Time page.
-->
<script lang="ts">
  import { untrack } from "svelte";
  import { ArrowDown01Icon, ArrowUp01Icon, LockIcon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import { preferences } from "$domain/preferences";
  import {
    TimeSession,
    duration,
    hourLabel,
    liveBucket,
    weekdayLetter,
    type Span,
  } from "$domain/time";
  import Bars from "$shared/ui/data/Bars";
  import Button from "$shared/ui/Button";
  import EmptyState from "$shared/ui/EmptyState";
  import Icon from "$shared/ui/Icon";
  import {
    STEADY_SECONDS,
    columns,
    comparison,
    dailyAverage,
    describeBucket,
    totals,
  } from "../lib/overview";
  import FocusCard from "./FocusCard.svelte";
  import SiteList from "./SiteList.svelte";

  let {
    profile,
    span,
    privateWindow = false,
    onopen,
  }: {
    profile: string;
    span: Span;
    privateWindow?: boolean;
    /** Opens the Time page. */
    onopen: () => void;
  } = $props();

  let session = $state.raw(untrack(() => new TimeSession(profile, span)));
  let pointed = $state<string | null>(null);

  $effect(() => {
    const owner = profile;
    return untrack(() => {
      const current = new TimeSession(owner, span);
      session = current;
      current.start();
      return () => current.stop();
    });
  });

  $effect(() => {
    const next = span;
    untrack(() => session.show(next));
  });

  let tracking = $derived(preferences.value("time.track") !== "false");
  let report = $derived(session.report);
  let sum = $derived(totals(report?.buckets ?? []));
  let values = $derived(columns(report?.buckets ?? []));
  let live = $derived(liveBucket(session.period));
  let change = $derived(report ? comparison(session.period, sum, report.previous) : null);
  let average = $derived(dailyAverage(session.period, sum));
  let highlight = $derived.by(() => {
    const series = report?.sites.find((site) => site.site === pointed)?.series ?? [];
    return series.length > 0 ? series : null;
  });
  let ticks = $derived(
    session.period.span === "day"
      ? [0, 6, 12, 18].map((index) => ({ index, label: hourLabel(index) }))
      : values.map((_, index) => ({
          index,
          label: weekdayLetter(session.period.start + index),
        })),
  );
  let focusToday = $derived(session.focus.reduce((total, day) => total + day.seconds, 0));
  let changeLabel = $derived.by(() => {
    if (change === null) return null;
    const amount = duration(Math.abs(change));
    const day = session.period.span === "day";
    if (Math.abs(change) < STEADY_SECONDS) return day ? m.time_steady_day() : m.time_steady_week();
    if (change < 0)
      return day ? m.time_less_day({ duration: amount }) : m.time_less_week({ duration: amount });
    return day ? m.time_more_day({ duration: amount }) : m.time_more_week({ duration: amount });
  });
</script>

{#if privateWindow}
  <EmptyState title={m.time_private()} description={m.time_local_only()}>
    {#snippet icon()}<Icon icon={LockIcon} size={22} />{/snippet}
  </EmptyState>
{:else if !tracking}
  <EmptyState title={m.time_off_title()} description={m.time_off_help()}>
    {#snippet icon()}<Icon icon={LockIcon} size={22} />{/snippet}
    {#snippet action()}<Button
        variant="primary"
        shape="capsule"
        onclick={() => void preferences.set("time.track", "true")}>{m.time_turn_on()}</Button
      >{/snippet}
  </EmptyState>
{:else}
  <div class="panel">
    <section class="overview" aria-live="polite">
      <p class="total">{duration(sum.all)}</p>
      <p class="sub">
        {m.time_on_web()}{#if sum.work >= 60}<span class="dot" aria-hidden="true">·</span
          >{m.time_in_work({ duration: duration(sum.work) })}{/if}
      </p>
      {#if changeLabel !== null}
        <p class="change">
          {#if change !== null && Math.abs(change) >= STEADY_SECONDS}<Icon
              icon={change < 0 ? ArrowDown01Icon : ArrowUp01Icon}
              size={12}
            />{/if}{changeLabel}
        </p>
      {:else if average !== null}
        <p class="change">{m.time_daily_average({ duration: duration(average) })}</p>
      {/if}
    </section>

    <Bars
      {values}
      {highlight}
      {live}
      {average}
      {ticks}
      height={88}
      axis={false}
      label={session.period.span === "day" ? m.time_chart_day() : m.time_chart_week()}
      describe={(index) => describeBucket(session.period, index)}
      format={duration}
    />

    <FocusCard todaySeconds={span === "day" ? focusToday : 0} onmanage={onopen} />

    {#if session.failed}
      <p class="quiet">{m.time_unavailable()}</p>
    {:else if report && report.sites.length > 0}
      <section class="sites">
        <h3>{m.time_sites()}</h3>
        <SiteList
          sites={report.sites}
          total={sum.browse}
          limit={6}
          onpoint={(site) => (pointed = site)}
        />
        <button type="button" class="more" onclick={onopen}>{m.time_show_all()}</button>
      </section>
    {:else if report}
      <p class="quiet">{m.time_no_sites_help()}</p>
    {/if}

    <p class="local"><Icon icon={LockIcon} size={11} />{m.time_local_only()}</p>
  </div>
{/if}

<style>
  .panel {
    display: grid;
    gap: 18px;
    padding: 6px 12px 16px;
  }

  .overview {
    display: grid;
    gap: 2px;
    padding-inline: 2px;
  }

  .total {
    margin: 0;
    font-size: 32px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.025em;
    line-height: 1.15;
  }

  .sub {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .dot {
    margin-inline: 6px;
    color: var(--color-faint);
  }

  .change {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    justify-self: start;
    margin: 8px 0 0;
    padding: 2px 8px 2px 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }

  .sites {
    display: grid;
    gap: 4px;
  }

  h3 {
    margin: 0;
    padding-inline: 10px;
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .sites :global(.sites) {
    margin-inline: -2px;
  }

  .more {
    justify-self: start;
    margin-block-start: 2px;
    padding: 4px 10px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    cursor: pointer;
  }

  .more:hover {
    background: var(--color-fill);
    color: var(--color-text);
  }

  .more:focus-visible {
    outline: 2px solid var(--color-ring);
  }

  .quiet {
    margin: 0;
    padding-inline: 10px;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .local {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    margin: 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }
</style>
