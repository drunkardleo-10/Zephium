<!--
  Focus at rest offers a length and a start; running, it becomes a ring that
  empties toward the end of the phase. The second hand only ticks while the
  card is on screen and the window visible.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { Target02Icon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import { preferences } from "$domain/preferences";
  import { breakMinutes, countdown, duration, focus } from "$domain/time";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import Ring from "$shared/ui/data/Ring";
  import SegmentedControl from "$shared/ui/SegmentedControl";
  import Switch from "$shared/ui/Switch";

  let {
    todaySeconds = 0,
    size = "panel",
    onmanage,
  }: {
    /** Focus already finished today, not counting a running session. */
    todaySeconds?: number;
    size?: "panel" | "page";
    /** Opens the list of sites focus shuts. */
    onmanage?: () => void;
  } = $props();

  const LENGTHS = ["25", "50", "90"];

  let now = $state(Date.now());
  let confirming = $state(false);
  let session = $derived(focus.session());
  let shut = $derived(
    preferences
      .value("focus.blocked")
      .split("\n")
      .filter((site) => site.length > 0).length,
  );
  let minutes = $derived(
    LENGTHS.includes(preferences.value("focus.minutes"))
      ? preferences.value("focus.minutes")
      : "25",
  );
  let breaks = $derived(preferences.value("focus.breaks") === "true");
  let rest = $derived(breakMinutes(Number(minutes)));
  let resting = $derived(session !== null && session.phase !== "focus");
  let ringSize = $derived(size === "page" ? 168 : 132);
  let ends = $derived(
    session?.phase_ends_at
      ? new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(
          new Date(session.phase_ends_at),
        )
      : "",
  );
  let phaseLabel = $derived(
    session === null
      ? ""
      : session.phase === "focus"
        ? m.focus_phase_focus()
        : session.phase === "break"
          ? m.focus_phase_break()
          : m.focus_phase_long_break(),
  );
  let focusedToday = $derived(
    todaySeconds + (session === null ? 0 : focus.focusedSeconds(session, now)),
  );

  onMount(() => {
    const tick = () => {
      if (document.visibilityState === "visible") now = Date.now();
    };
    const timer = setInterval(tick, 1000);
    document.addEventListener("visibilitychange", tick);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", tick);
    };
  });

  function end() {
    if (!confirming) {
      confirming = true;
      return;
    }
    confirming = false;
    void focus.stop();
  }
</script>

<section class="focus" class:page={size === "page"} class:running={session !== null}>
  <header>
    <span class="mark"><Icon icon={Target02Icon} size={15} /></span>
    <h3>{m.focus_title()}</h3>
    {#if focusedToday >= 60}
      <span class="today">{m.focus_today({ duration: duration(focusedToday) })}</span>
    {/if}
  </header>

  {#if session === null}
    <p class="lede">{m.focus_idle()}</p>
    <div class="choose">
      <SegmentedControl
        label={m.focus_length()}
        full
        value={minutes}
        options={LENGTHS.map((value) => ({
          value,
          label: m.focus_minutes({ minutes: value }),
        }))}
        onchange={(value) => void preferences.set("focus.minutes", value)}
      />
      <div class="option">
        <span>
          <span class="option-title">{m.focus_breaks()}</span>
          {#if breaks}<span class="option-detail"
              >{m.focus_breaks_detail({
                short: String(rest.short),
                long: String(rest.long),
              })}</span
            >{/if}
        </span>
        <Switch
          label={m.focus_breaks()}
          labelHidden
          checked={breaks}
          disabled={preferences.saving()}
          onchange={(value) => void preferences.set("focus.breaks", String(value))}
        />
      </div>
    </div>
    <footer>
      <button type="button" class="shut" onclick={onmanage} disabled={!onmanage}>
        {shut === 0
          ? m.focus_none_shut()
          : shut === 1
            ? m.focus_shut_one()
            : m.focus_shut_count({ count: shut })}
      </button>
      <Button
        variant="primary"
        shape="capsule"
        pending={focus.busy()}
        onclick={() => void focus.start(Number(minutes), breaks)}>{m.focus_start()}</Button
      >
    </footer>
  {:else}
    <div class="live">
      <Ring
        value={1 - focus.progress(session, now)}
        size={ringSize}
        stroke={size === "page" ? 9 : 7}
        tone={resting ? "rest" : "lead"}
        label={phaseLabel}
      >
        <span class="clock">{countdown(focus.remaining(session, now))}</span>
        <span class="phase">{phaseLabel}</span>
      </Ring>
      <div class="facts">
        <span class="round"
          >{m.focus_round({ round: String(session.rounds + (resting ? 0 : 1)) })}</span
        >
        <span class="until">{m.focus_until({ time: ends })}</span>
        {#if !resting && shut > 0}
          <button type="button" class="shut" onclick={onmanage} disabled={!onmanage}>
            {shut === 1 ? m.focus_shut_one() : m.focus_shut_count({ count: shut })}
          </button>
        {/if}
        <div class="actions">
          {#if resting}
            <Button size="compact" shape="capsule" onclick={() => void focus.skip()}
              >{m.focus_skip_break()}</Button
            >
          {/if}
          <Button
            size="compact"
            shape="capsule"
            variant={confirming ? "danger" : "ghost"}
            onblur={() => (confirming = false)}
            onclick={end}>{confirming ? m.focus_end_confirm() : m.focus_end()}</Button
          >
        </div>
      </div>
    </div>
  {/if}
  {#if focus.lastFailed()}<p class="failed" role="alert">{m.focus_failed()}</p>{/if}
</section>

<style>
  .focus {
    display: grid;
    gap: 12px;
    padding: 14px;
    border-radius: var(--radius-card);
    background: var(--color-card);
    box-shadow: var(--row-rim);
  }

  .focus.page {
    gap: 16px;
    padding: 20px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .mark {
    display: inline-grid;
    place-items: center;
    inline-size: 26px;
    block-size: 26px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    color: var(--color-text);
  }

  .running .mark {
    background: var(--color-lit);
    color: var(--color-on-lit);
  }

  h3 {
    margin: 0;
    font-size: var(--text-body);
    font-weight: 600;
  }

  .today {
    margin-inline-start: auto;
    color: var(--color-muted);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .lede {
    margin: -4px 0 0;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .choose {
    display: grid;
    gap: 10px;
  }

  .option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 30px;
  }

  .option > span {
    display: grid;
    gap: 1px;
    min-width: 0;
  }

  .option-title {
    font-size: var(--text-body);
  }

  .option-detail {
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .shut {
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    text-align: start;
    cursor: pointer;
  }

  .shut:disabled {
    cursor: default;
  }

  .shut:focus-visible {
    border-radius: 4px;
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .shut:hover:not(:disabled) {
    color: var(--color-text);
  }

  .live {
    display: flex;
    align-items: center;
    gap: 18px;
    animation: settle var(--motion-page) var(--ease-emphasized);
  }

  .clock {
    font-size: 26px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
    line-height: 1.1;
  }

  .page .clock {
    font-size: 34px;
  }

  .phase {
    margin-block-start: 2px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .facts {
    display: grid;
    gap: 4px;
    min-width: 0;
    justify-items: start;
  }

  .round {
    font-size: var(--text-body);
    font-weight: 600;
  }

  .until {
    color: var(--color-muted);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-block-start: 8px;
  }

  .failed {
    margin: 0;
    color: var(--color-danger);
    font-size: var(--text-label);
  }

  @keyframes settle {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .live {
      animation: none;
    }
  }

  :global(:root[data-reduce-motion="true"]) .live {
    animation: none;
  }
</style>
