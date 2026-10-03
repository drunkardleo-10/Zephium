<!--
  Focus is one dial. At rest its centre is the length, which opens the
  choices; running, it empties toward the end of the phase. The layout stays
  put between the two, so starting reads as the dial coming alive.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { ArrowDown01Icon } from "@hugeicons/core-free-icons";
  import type { IconRef } from "$shared/ipc/bindings";
  import * as m from "$shared/i18n/messages";
  import { preferences } from "$domain/preferences";
  import { countdown, focus } from "$domain/time";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import Menu, { type MenuEntry } from "$shared/ui/Menu";
  import Ring from "$shared/ui/data/Ring";
  import ShutSites from "./ShutSites.svelte";

  let {
    size = "panel",
    suggestions = [],
    onmanage,
  }: {
    size?: "panel" | "page";
    /** Sites to offer for shutting, from the reader's own time. */
    suggestions?: { site: string; icon: IconRef | null }[];
    /** Where the shut sites are managed; without it they open in place. */
    onmanage?: () => void;
  } = $props();

  const LENGTHS = [15, 25, 30, 45, 50, 60, 90];

  let now = $state(Date.now());
  let confirming = $state(false);
  let editing = $state(false);
  let session = $derived(focus.session());
  let minutes = $derived(Number(preferences.value("focus.minutes")) || 25);
  let breaks = $derived(preferences.value("focus.breaks") === "true");
  let resting = $derived(session !== null && session.phase !== "focus");
  let dial = $derived(size === "page" ? 148 : 112);
  let shut = $derived(
    preferences
      .value("focus.blocked")
      .split("\n")
      .filter((site) => site.length > 0).length,
  );
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
  let choices = $derived<MenuEntry[]>([
    ...LENGTHS.map((length) => ({
      kind: "item" as const,
      id: String(length),
      label: m.focus_minutes({ minutes: String(length) }),
      checked: length === minutes,
    })),
    { kind: "separator" },
    { kind: "item", id: "breaks", label: m.focus_breaks(), checked: breaks },
  ]);
  let shutLabel = $derived(
    shut === 0
      ? m.focus_none_shut()
      : shut === 1
        ? m.focus_shut_one()
        : m.focus_shut_count({ count: shut }),
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

  function choose(id: string) {
    if (id === "breaks") void preferences.set("focus.breaks", String(!breaks));
    else void preferences.set("focus.minutes", id);
  }

  function manage() {
    if (onmanage) onmanage();
    else editing = !editing;
  }

  function end() {
    if (!confirming) {
      confirming = true;
      return;
    }
    confirming = false;
    void focus.stop();
  }
</script>

<section class="focus" class:page={size === "page"} aria-label={m.focus_title()}>
  <div class="body">
    {#if session === null}
      <div class="dial">
        <Ring value={0} size={dial} stroke={size === "page" ? 8 : 6} />
        <Menu
          label={m.focus_length()}
          entries={choices}
          side="bottom"
          align="start"
          triggerClass="focus-length"
          onselect={choose}
        >
          {#snippet trigger()}<span class="length">{minutes}</span><span class="unit"
              >min<Icon icon={ArrowDown01Icon} size={11} /></span
            >{/snippet}
        </Menu>
      </div>
      <div class="facts">
        <h3>{m.focus_title()}</h3>
        <button
          type="button"
          class="shut"
          aria-expanded={onmanage ? undefined : editing}
          onclick={manage}>{shutLabel}</button
        >
        <div class="actions">
          <Button
            variant="primary"
            shape="capsule"
            size={size === "page" ? "regular" : "compact"}
            pending={focus.busy()}
            onclick={() => void focus.start(minutes, breaks)}>{m.focus_start()}</Button
          >
        </div>
      </div>
    {:else}
      <div class="dial live">
        <Ring
          value={1 - focus.progress(session, now)}
          size={dial}
          stroke={size === "page" ? 8 : 6}
          tone={resting ? "rest" : "lead"}
          label={phaseLabel}
        >
          <span class="clock">{countdown(focus.remaining(session, now))}</span>
        </Ring>
      </div>
      <div class="facts">
        <h3>{phaseLabel}</h3>
        <span class="detail"
          >{m.focus_round({ round: String(session.rounds + (resting ? 0 : 1)) })} · {m.focus_until({
            time: ends,
          })}</span
        >
        {#if !resting && shut > 0}
          <button
            type="button"
            class="shut"
            aria-expanded={onmanage ? undefined : editing}
            onclick={manage}>{shutLabel}</button
          >
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
            variant={confirming ? "danger" : "secondary"}
            onblur={() => (confirming = false)}
            onclick={end}>{confirming ? m.focus_end_confirm() : m.focus_end()}</Button
          >
        </div>
      </div>
    {/if}
  </div>
  {#if editing && !onmanage}
    <div class="editor"><ShutSites {suggestions} /></div>
  {/if}
  {#if focus.lastFailed()}<p class="failed" role="alert">{m.focus_failed()}</p>{/if}
</section>

<style>
  .focus {
    display: grid;
    gap: 14px;
    padding: 14px;
    border-radius: var(--radius-card);
    background: var(--color-card);
    box-shadow: var(--row-rim);
  }

  .focus.page {
    padding: 20px;
  }

  .body {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .dial {
    position: relative;
    display: grid;
    place-items: center;
    flex: none;
  }

  .dial > :global(.focus-length) {
    position: absolute;
    inset: 50% auto auto 50%;
    display: grid;
    justify-items: center;
    gap: 0;
    padding: 6px 10px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    cursor: default;
    translate: -50% -50%;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .dial > :global(.focus-length:hover),
  .dial > :global(.focus-length[data-state="open"]) {
    background: var(--color-fill-hover);
  }

  .dial > :global(.focus-length:focus-visible) {
    outline: 2px solid var(--color-ring);
  }

  .length {
    font-size: 30px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.03em;
    line-height: 1;
  }

  .page .length {
    font-size: 38px;
  }

  .unit {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .clock {
    font-size: 24px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
  }

  .page .clock {
    font-size: 30px;
  }

  .live {
    animation: settle var(--motion-page) var(--ease-emphasized);
  }

  .facts {
    display: grid;
    gap: 3px;
    justify-items: start;
    min-width: 0;
  }

  h3 {
    margin: 0;
    font-size: var(--text-page-title);
    font-weight: 600;
  }

  .detail {
    color: var(--color-muted);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
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

  .shut:focus-visible {
    border-radius: 4px;
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .shut:hover {
    color: var(--color-text);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-block-start: 10px;
  }

  .editor {
    padding-block-start: 12px;
    border-block-start: 1px solid var(--color-border);
    animation: settle var(--motion-slow) var(--ease-out);
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
    .live,
    .editor {
      animation: none;
    }
  }

  :global(:root[data-reduce-motion="true"]) :is(.live, .editor) {
    animation: none;
  }
</style>
