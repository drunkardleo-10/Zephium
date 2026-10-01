<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import {
    CheckListIcon,
    Clock01Icon,
    Settings01Icon,
    Shield01Icon,
    UserCircleIcon,
  } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import { events } from "$shared/ipc/native-events";
  import { preferences } from "$domain/preferences";
  import { taskCounts } from "$domain/resources";
  import { tabs } from "$domain/tabs";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
  import { greetingFor } from "../lib/greeting";
  import { clockFace, clockFormat, dayKey, focusSpan, untilNextMinute } from "../lib/clock";
  import { SAMPLE_DAY } from "../lib/figures";
  import { groundPath, notchShape, notchWidth, roundedRect } from "../lib/ground";
  import { layout, type Box } from "../lib/layout";
  import { WORDMARK } from "$shared/lib/wordmark";

  let {
    search,
    oncustomize,
    onprofile,
    ontasks,
    trackersBlocked = SAMPLE_DAY.trackersBlocked,
    focusMinutes = SAMPLE_DAY.focusMinutes,
    focusGoalMinutes = SAMPLE_DAY.focusGoalMinutes,
  }: {
    /** The field that hangs in the notch. */
    search: Snippet;
    /** Opens this page's settings. */
    oncustomize?: () => void;
    /** Opens the profile's settings. */
    onprofile?: () => void;
    /** Opens Tasks. */
    ontasks?: () => void;
    /** Trackers the blocker stopped today. */
    trackersBlocked?: number;
    /** Minutes spent focused today, and the day's aim. */
    focusMinutes?: number;
    focusGoalMinutes?: number;
  } = $props();

  const id = $props.id();

  let width = $state(0);
  let height = $state(0);
  let now = $state(new Date());
  let due = $state({ today: 0, overdue: 0 });

  let incognito = $derived(tabs.profile()?.kind === "incognito");
  let profile = $derived(tabs.profile()?.id ?? null);
  let showGreeting = $derived(preferences.value("ui.newtab-greeting") === "true");
  let showClock = $derived(preferences.value("ui.newtab-clock") === "true");
  let showDue = $derived(!incognito && preferences.value("ui.newtab-tasks") === "true");
  let today = $derived(dayKey(now));

  let firstName = $derived((tabs.profile()?.name ?? "").trim().split(/\s+/u)[0] ?? "");
  let greeting = $derived.by(() => {
    const words = greetingFor(now);
    const named = !incognito && preferences.value("ui.newtab-name") === "true" && firstName;
    return named ? m.ntp_greeting_named({ greeting: words, name: firstName }) : words;
  });
  let face = $derived(clockFace(now, clockFormat(preferences.value("ui.newtab-clock-format"))));
  let date = $derived(
    new Intl.DateTimeFormat(undefined, { weekday: "long", month: "long", day: "numeric" }).format(
      now,
    ),
  );
  const number = new Intl.NumberFormat();
  let focusShare = $derived(
    focusGoalMinutes > 0 ? Math.min(1, Math.max(0, focusMinutes / focusGoalMinutes)) : 0,
  );

  // The figures a private window shows none of; what is due only while Tasks
  // is wanted here.
  let figures = $derived(
    incognito ? [] : showDue ? ["trackers", "focus", "due"] : ["trackers", "focus"],
  );
  let controls = $derived((onprofile ? 1 : 0) + (oncustomize ? 1 : 0));
  let page = $derived(layout(width, height, { controls, clock: showClock, tiles: figures.length }));

  // One outline for the ground: the page less the notch, the controls' panes,
  // and the name's box, which a patch of ground fills back in less the
  // letters themselves.
  const pane = ({ x, y, w, h }: Box, r: number) => roundedRect(x, y, w, h, r);
  const at = ({ x, y, w, h }: Box) => `left:${x}px;top:${y}px;width:${w}px;height:${h}px`;
  let openings = $derived(page.controls.map((control) => pane(control, control.h / 2)));
  let box = $derived(pane(page.mark, 0));
  // Reaches a little past the box, so the patch and the hole never meet edge
  // to edge and leave a hairline where both are antialiased.
  let patch = $derived(
    pane({ x: page.mark.x - 2, y: page.mark.y - 2, w: page.mark.w + 4, h: page.mark.h + 4 }, 0),
  );
  let ground = $derived(groundPath(width, height, page.notch, [...openings, box]));
  let lip = $derived(notchShape(width, height, page.notch));
  let fieldWidth = $derived(notchWidth(width, page.notch));
  let letters = $derived(
    `translate(${page.mark.x} ${page.mark.y}) scale(${page.mark.w / WORDMARK.width}) ` +
      `translate(${-WORDMARK.x} ${-WORDMARK.y}) scale(${WORDMARK.scaleX} ${WORDMARK.scaleY})`,
  );

  // What is due today, overdue included, kept current while the page shows it:
  // read once, then again whenever a task in this profile changes.
  $effect(() => {
    const owner = profile;
    const day = today;
    if (!owner || !showDue) {
      due = { today: 0, overdue: 0 };
      return;
    }
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const read = () =>
      void taskCounts(owner, day).then(
        (counts) => {
          if (live && counts) due = { today: counts.today, overdue: counts.overdue };
        },
        () => {},
      );
    read();
    const listening = events.resourceChanged.listen(({ payload }) => {
      if (payload.profile !== owner || payload.kind !== "task") return;
      clearTimeout(timer);
      timer = setTimeout(read, 250);
    });
    return () => {
      live = false;
      clearTimeout(timer);
      void listening.then((stop) => stop());
    };
  });

  onMount(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    // One redraw per minute, on the turn, and none while the page is hidden.
    function update() {
      now = new Date();
      clearTimeout(timer);
      if (document.visibilityState === "visible")
        timer = setTimeout(update, untilNextMinute(Date.now()));
    }
    function visibility() {
      clearTimeout(timer);
      if (document.visibilityState === "visible") update();
    }
    update();
    document.addEventListener("visibilitychange", visibility);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", visibility);
    };
  });
</script>

<!--
  Drawn by the chrome, so a new tab costs no WebView and shows no paint flash.
  The page is solid, as every page is, and the window shows through it in
  three places: the notch the field hangs in, the letters of the name, and
  the panes of the page's controls. The day's figures are cards along its
  foot.
  Everything is placed in the page's own pixels from one layout, so the cuts
  and what stands in them move as one.
-->
<section
  class="newtab"
  aria-label={m.new_tab()}
  bind:clientWidth={width}
  bind:clientHeight={height}
>
  <svg class="ground" aria-hidden="true">
    <defs>
      <path id={`${id}-letters`} d={WORDMARK.d} transform={letters} />
      <!-- Only as large as the name, so the offscreen buffer a mask costs is
           the size of the letters and not of the page. -->
      <mask
        id={`${id}-cut`}
        maskUnits="userSpaceOnUse"
        x={page.mark.x - 2}
        y={page.mark.y - 2}
        width={page.mark.w + 4}
        height={page.mark.h + 4}
      >
        <path d={patch} fill="white" />
        <use href={`#${id}-letters`} fill="black" />
      </mask>
      <!-- Light caught along the edge a pane shows the page, fading away
           from it. -->
      <linearGradient id={`${id}-lip-up`} x1="0" y1="1" x2="0" y2="0">
        <stop offset="0" class="lip" />
        <stop offset="0.6" class="lip" stop-opacity="0" />
      </linearGradient>
      <linearGradient id={`${id}-lip-down`} x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" class="lip" />
        <stop offset="0.5" class="lip" stop-opacity="0" />
      </linearGradient>
      <linearGradient id={`${id}-wash`} x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" class="wash-lit" />
        <stop offset="1" class="wash-dim" />
      </linearGradient>
    </defs>
    <path class="page" fill-rule="evenodd" d={ground} />
    <path class="page" d={patch} mask={`url(#${id}-cut)`} />
    <use class="solid" href={`#${id}-letters`} />
    <use class="wash" href={`#${id}-letters`} fill={`url(#${id}-wash)`} />
    <path class="rim" d={lip} stroke={`url(#${id}-lip-up)`} />
    {#each openings as opening, index (index)}
      <path class="pane" d={opening} stroke={`url(#${id}-lip-down)`} />
    {/each}
  </svg>

  <div class="field" style:width={`${fieldWidth}px`} style:height={`${page.notch.height}px`}>
    {@render search()}
  </div>

  <!-- Each control in its own round pane of the window, level with the
       field. -->
  {#if onprofile && page.controls[0]}
    <div class="control" data-glass-text style={at(page.controls[0])}>
      <IconButton
        icon={UserCircleIcon}
        label={m.ntp_profile()}
        shape="circle"
        size={16}
        buttonSize={32}
        onclick={onprofile}
      />
    </div>
  {/if}
  {#if oncustomize && page.controls.at(-1)}
    <div class="control" data-glass-text style={at(page.controls.at(-1)!)}>
      <IconButton
        icon={Settings01Icon}
        label={m.settings_customize_newtab()}
        shape="circle"
        size={16}
        buttonSize={32}
        onclick={oncustomize}
      />
    </div>
  {/if}

  {#if showGreeting && !incognito}
    <p class="greeting" style:bottom={`${height - page.mark.y + 24}px`}>{greeting}</p>
  {/if}
  <!-- The letters are drawn by the ground; this stands where they are so the
       name has a place and a label. -->
  <span class="mark" role="img" aria-label="Zephium" style={at(page.mark)}></span>

  {#if page.when !== null}
    <p class="when" style:top={`${page.when}px`}>
      <time datetime={now.toISOString()}
        >{#if face.period !== null && face.periodLeading}<span class="period"
            >{`${face.period} `}</span
          >{/if}{face.time}{#if face.period !== null && !face.periodLeading}<span class="period"
            >{` ${face.period}`}</span
          >{/if}</time
      ><span class="date">{date}</span>
    </p>
  {/if}

  {#if incognito}
    <p class="note" style:top={`${(page.when ?? page.mark.y + page.mark.h) + 44}px`}>
      {m.ntp_private()}
    </p>
  {/if}

  <!-- The day in figures, a card each, along the foot of the page: what it
       is, then the figure with its context read on the same line. -->
  {#each figures as figure, index (figure)}
    {@const tile = page.tiles[index]}
    {#if tile}
      {#if figure === "trackers"}
        <div class="tile" style={at(tile)}>
          <span class="label"
            ><Icon icon={Shield01Icon} size={13} /><span class="name"
              >{m.ntp_trackers_blocked()}</span
            ></span
          >
          <span class="reading">
            <span class="value">{number.format(trackersBlocked)}</span>
            <span class="foot">{m.ntp_today()}</span>
          </span>
        </div>
      {:else if figure === "focus"}
        <div class="tile" style={at(tile)}>
          <span class="label"
            ><Icon icon={Clock01Icon} size={13} /><span class="name">{m.ntp_focused()}</span><span
              class="meter"
              role="meter"
              aria-label={m.ntp_focus_goal({ goal: focusSpan(focusGoalMinutes) })}
              aria-valuemin={0}
              aria-valuemax={focusGoalMinutes}
              aria-valuenow={Math.min(focusMinutes, focusGoalMinutes)}
              ><span class="fill" style:transform={`scaleX(${focusShare})`}></span></span
            ></span
          >
          <span class="reading">
            <span class="value">{focusSpan(focusMinutes)}</span>
            <span class="foot">{m.ntp_focus_goal({ goal: focusSpan(focusGoalMinutes) })}</span>
          </span>
        </div>
      {:else}
        <button type="button" class="tile" style={at(tile)} onclick={ontasks}>
          <span class="label"
            ><Icon icon={CheckListIcon} size={13} /><span class="name">{m.ntp_due_today()}</span
            ></span
          >
          <span class="reading">
            <span class="value">{due.today}</span>
            <span class="foot" class:late={due.overdue > 0}
              >{due.overdue > 0
                ? m.ntp_overdue({ count: due.overdue })
                : m.ntp_nothing_overdue()}</span
            >
          </span>
        </button>
      {/if}
    {/if}
  {/each}
</section>

<style>
  /* Whatever the ground leaves open is the window: the chrome's own colour on
     an opaque window, and nothing where the window has a material to show.
     Everything here is placed in the drawing's physical pixels, which do not
     mirror in a right-to-left page. */
  .newtab {
    --mark-lit: 42%;
    --mark-dim: 18%;
    --tile-hover: var(--color-fill-hover);

    position: relative;
    block-size: 100%;
    overflow: hidden;
    background: var(--color-chrome);
  }

  :global(:root[data-theme="light"]) .newtab {
    --mark-lit: 64%;
    --mark-dim: 46%;
    --tile-hover: var(--color-control-hover);
  }

  :global(:root:not([data-material="none"])[data-material]) .newtab {
    background: transparent;
  }

  .ground {
    position: absolute;
    inset: 0;
    inline-size: 100%;
    block-size: 100%;
    pointer-events: none;
  }

  .page {
    fill: var(--color-page);
  }

  /* On an opaque window there is nothing behind the letters worth showing,
     so they are drawn in the text colour over the cut. Over a material they
     are the material, with a light wash so the name still reads first. */
  .solid {
    fill: var(--color-text);
  }

  .wash {
    display: none;
  }

  :global(:root:not([data-material="none"])[data-material]) .solid {
    display: none;
  }

  :global(:root:not([data-material="none"])[data-material]) .wash {
    display: inline;
  }

  .wash-lit {
    stop-color: color-mix(in srgb, var(--color-text) var(--mark-lit), transparent);
  }

  .wash-dim {
    stop-color: color-mix(in srgb, var(--color-text) var(--mark-dim), transparent);
  }

  /* The notch is the frame itself and stays clear; the controls' panes
     carry the launcher's wash, so they read as glass set into the page
     rather than holes in it. */
  .rim,
  .pane {
    stroke-width: 1;
  }

  .rim {
    fill: none;
  }

  .pane {
    fill: color-mix(in srgb, var(--color-chrome) var(--wash-launcher), transparent);
  }

  .lip {
    stop-color: var(--color-border-strong);
  }

  .field {
    position: absolute;
    z-index: 1;
    inset-block-start: 0;
    inset-inline: 0;
    margin-inline: auto;
  }

  .control {
    position: absolute;
    display: grid;
    place-items: center;
    color: var(--color-muted);
  }

  .mark {
    position: absolute;
    pointer-events: none;
  }

  .greeting,
  .when,
  .note {
    position: absolute;
    inset-inline: 0;
    margin: 0 auto;
    text-align: center;
  }

  .greeting {
    color: var(--color-faint);
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.16em;
    text-transform: uppercase;
  }

  .when {
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 10px;
    color: var(--color-muted);
    font-size: 14px;
    line-height: 18px;
  }

  .when time {
    color: var(--color-text);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  .period {
    color: var(--color-muted);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
  }

  .date::before {
    content: "";
    display: inline-block;
    inline-size: 3px;
    block-size: 3px;
    margin-inline-end: 10px;
    border-radius: var(--radius-capsule);
    background: currentcolor;
    vertical-align: middle;
    opacity: 0.6;
  }

  .note {
    max-inline-size: 36ch;
    color: var(--color-faint);
    font-size: var(--text-label);
    line-height: 1.5;
  }

  /* A card on the page: what it is, and under it the figure with its
     context on the same line, on a card fill with the controls' rim. */
  .tile {
    position: absolute;
    display: flex;
    box-sizing: border-box;
    flex-direction: column;
    justify-content: space-between;
    container: tile / inline-size;
    padding: 13px 16px 12px;
    border: 0;
    border-radius: var(--radius-card);
    background: var(--color-card);
    box-shadow: var(--shadow-raise);
    color: inherit;
    font: inherit;
    text-align: start;
    animation: arrive var(--motion-page) var(--ease-emphasized) 60ms backwards;
  }

  button.tile {
    cursor: default;
    outline: none;
    transition:
      background-color var(--motion-instant) var(--ease-smooth),
      scale var(--motion-slow) var(--ease-smooth);
  }

  button.tile:hover {
    background: var(--tile-hover);
  }

  button.tile:active {
    scale: 0.985;
    transition-duration: var(--motion-fast);
  }

  button.tile:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .label {
    display: flex;
    align-items: center;
    gap: 6px;
    min-inline-size: 0;
    overflow: hidden;
    color: var(--color-muted);
    font-size: 12px;
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
  }

  .label :global(svg) {
    flex: none;
    color: var(--color-faint);
  }

  .name {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .reading {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    min-inline-size: 0;
  }

  .value {
    flex: none;
    color: var(--color-text);
    font-size: 22px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    line-height: 26px;
    letter-spacing: -0.025em;
  }

  .foot {
    display: flex;
    align-items: center;
    min-inline-size: 0;
    overflow: hidden;
    color: var(--color-faint);
    font-size: 11.5px;
    font-weight: 500;
    white-space: nowrap;
  }

  .foot.late {
    color: var(--color-danger);
  }

  /* The day's progress toward the aim, at the far end of the label. */
  .meter {
    position: relative;
    flex: none;
    inline-size: 36px;
    margin-inline-start: auto;
    block-size: 3px;
    overflow: hidden;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-strong);
  }

  .fill {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: var(--color-text);
    transform-origin: left center;
  }

  /* A card too narrow for the figure and its context keeps the figure on
     screen; the context is still read out. */
  @container tile (inline-size < 148px) {
    .foot,
    .meter {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
    }
  }

  /* The ground is there from the first frame, so the page never shows the
     window through itself; only what stands on it fades in. */
  .control,
  .greeting,
  .when {
    animation: arrive var(--motion-page) var(--ease-emphasized) backwards;
  }

  @keyframes arrive {
    from {
      opacity: 0;
    }
  }

  @media (forced-colors: active) {
    .solid {
      fill: CanvasText;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .control,
    .greeting,
    .when,
    .tile {
      animation: none;
    }
  }

  :global(:root[data-reduce-motion="true"]) :is(.control, .greeting, .when, .tile) {
    animation: none;
  }
</style>
