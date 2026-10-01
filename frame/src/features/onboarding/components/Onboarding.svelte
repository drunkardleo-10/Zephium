<script lang="ts">
  import { onMount, untrack, type Snippet } from "svelte";
  import { CheckListIcon, Note01Icon, PuzzleIcon, Shield01Icon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import { browserImport } from "$domain/browser-import";
  import { tabs } from "$domain/tabs";
  import { commands } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import KeyHint from "$shared/ui/KeyHint";
  import { IS_MAC } from "$shared/platform";
  import { reducedMotion } from "$shared/lib/motion";
  import { WORDMARK } from "$shared/lib/wordmark";
  import * as motion from "$session/motion.svelte";
  import { SITES, SITE_URLS } from "../lib/catalog";
  import { frame, SHOTS, STAGE, type Shot } from "../lib/camera";
  import { neighbour, position, STEPS, type Step } from "../lib/flow";
  import { formName, type Box } from "../lib/formation";
  import { keptSites } from "../lib/kept";
  import Window from "./Window.svelte";
  import You from "./steps/You.svelte";
  import Import from "./steps/Import.svelte";
  import Essentials from "./steps/Essentials.svelte";
  import Launcher from "./steps/Launcher.svelte";

  let {
    onfinish,
    windowControls,
  }: {
    /** Hands the window to the browser; false if native refused. */
    onfinish: () => Promise<boolean>;
    windowControls?: Snippet;
  } = $props();

  let step = $state<Step>("welcome");
  /** Welcome's own beats: the air gathering, the name standing, and the
   *  air gone, with nothing of it left running. */
  let phase = $state<"waiting" | "forming" | "formed" | "still">("waiting");
  let leaving = $state(false);
  let notice = $state<string | null>(null);

  let width = $state(0);
  let height = $state(0);
  let fit = $derived(width && height ? Math.min(width / STAGE.width, height / STAGE.height) : 1);

  let air = $state<HTMLCanvasElement>();
  let windowElement = $state<HTMLElement>();
  let flights = $state<HTMLElement>();

  const current = tabs.profile()?.name ?? "";
  // Asked for and kept, and for now shown nowhere.
  let name = $state(current === "Personal" ? "" : current);

  let kept = $derived(keptSites(tabs.sidebarNodes(), tabs.tabs(), SITE_URLS));
  let sites = $derived([...kept.keys()].flatMap((id) => SITES.filter((site) => site.id === id)));
  let imported = $derived.by(() => {
    const job = browserImport.current();
    if (!job?.finished) return null;
    const from = browserImport.found()?.find((source) => source.id === job.source)?.name ?? "";
    const bookmarks = job.kinds.find((entry) => entry.kind === "bookmarks")?.done ?? 0;
    return { from, bookmarks };
  });

  let shot = $derived<Shot>(step === "welcome" ? SHOTS.peek : SHOTS[step]);
  // The fade is masked one scene past the last shot that fades, by then fully
  // open, so taking the mask away never shows as a jump.
  let masked = $derived(step === "welcome" || step === "you" || step === "import");
  let place = $derived(position(step));

  const box: Box = { x: 340, y: 266, w: 560, h: (560 * WORDMARK.height) / WORDMARK.width };

  // The name forms once the window is actually on screen, never into a
  // window native has not revealed yet.
  let revealed = $derived(motion.launchState() !== "armed");
  // Started by the reveal alone: nothing it reads later, the stage's scale
  // included, may restart or cut short a formation already under way.
  $effect(() => {
    if (!revealed || !air) return;
    return untrack(() => {
      if (phase !== "waiting" || !air) return;
      phase = "forming";
      void commands.onboardingPlayIntro().catch(() => {});
      if (reducedMotion()) {
        phase = "still";
        return;
      }
      const color = getComputedStyle(document.documentElement).getPropertyValue("--color-text");
      return formName(air, {
        stage: STAGE,
        box,
        path: new Path2D(WORDMARK.d),
        place: (context) => {
          context.scale(box.w / WORDMARK.width, box.h / WORDMARK.height);
          context.translate(-WORDMARK.x, -WORDMARK.y);
          context.scale(WORDMARK.scaleX, WORDMARK.scaleY);
        },
        density: Math.min(2, window.devicePixelRatio || 1) * Math.min(1.5, fit),
        color: color.trim() || "#f4f4f6",
        onformed: () => (phase = "formed"),
        ondone: () => (phase = "still"),
      });
    });
  });

  function begin() {
    if (step !== "welcome" || (phase !== "formed" && phase !== "still")) return;
    step = "you";
  }

  function go(next: Step | undefined) {
    if (!next || leaving) return;
    if (step === "you") saveName();
    step = next;
  }

  function saveName() {
    const chosen = name.trim();
    if (chosen && chosen !== current) void tabs.renameProfile(chosen);
  }

  async function finish() {
    if (leaving) return;
    if (step === "you") saveName();
    leaving = true;
    // The scene steps back to nothing first; the browser then arrives in the
    // window it leaves, with its own launch.
    await new Promise((resolve) => setTimeout(resolve, reducedMotion() ? 0 : 1000));
    if (await onfinish()) return;
    leaving = false;
    say(m.onb_finish_failed());
  }

  const primary = $derived.by((): string => {
    if (step === "you") return name.trim() ? m.onb_continue() : m.onb_not_now();
    if (step === "import") return imported ? m.onb_continue() : m.onb_skip_step();
    if (step === "ready") return m.onb_start();
    return m.onb_continue();
  });
  const advance = () => (step === "ready" ? void finish() : go(neighbour(step, 1)));

  function keydown(event: KeyboardEvent) {
    if (event.key !== "Enter" || event.repeat || event.isComposing) return;
    if ((event.target as HTMLElement | null)?.closest("button, a, input")) return;
    event.preventDefault();
    if (step === "welcome") begin();
    else advance();
  }

  const headings: Partial<Record<Step, { title: () => string; body?: () => string }>> = {
    you: { title: m.onb_you_title },
    import: { title: m.onb_import_title, body: m.onb_import_body },
    essentials: { title: m.onb_essentials_title, body: m.onb_essentials_body },
    launcher: { title: m.onb_launcher_title, body: m.onb_launcher_body },
    work: { title: m.onb_work_title, body: m.onb_work_body },
    ready: { title: m.onb_ready_title },
  };

  // What is already on before anything is set: the reasons to stay.
  const features = [
    { icon: Shield01Icon, label: m.onb_ready_blocking },
    { icon: PuzzleIcon, label: m.onb_ready_extensions },
    { icon: CheckListIcon, label: m.onb_ready_tasks },
    { icon: Note01Icon, label: m.onb_ready_notes },
  ];

  function landing(site: string) {
    const target =
      windowElement?.querySelector(`[data-kept="${CSS.escape(site)}"]`) ??
      windowElement?.querySelector("[data-kept-more]");
    return target?.getBoundingClientRect() ?? null;
  }

  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  function say(text: string) {
    notice = text;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), 3200);
  }
  const failed = (site: string) => say(m.onb_essentials_failed({ site }));

  onMount(() => {
    void browserImport.detect();
    return () => clearTimeout(noticeTimer);
  });
</script>

<svelte:window onkeydown={keydown} />

<div
  class="onboarding"
  role="dialog"
  aria-modal="true"
  aria-label={m.onb_label()}
  data-step={step}
  data-phase={phase}
  data-leaving={leaving}
  bind:clientWidth={width}
  bind:clientHeight={height}
>
  <div class="ground" data-tauri-drag-region></div>

  <div class="stage" style:transform={`translate(-50%, -50%) scale(${fit})`}>
    {#if phase !== "still"}<canvas class="air" bind:this={air} aria-hidden="true"></canvas>{/if}
    <svg
      class="name"
      viewBox="{WORDMARK.x} {WORDMARK.y} {WORDMARK.width} {WORDMARK.height}"
      style:left={`${box.x}px`}
      style:top={`${box.y}px`}
      style:width={`${box.w}px`}
      style:height={`${box.h}px`}
      role="img"
      aria-label="Zephium"
      ><path d={WORDMARK.d} transform="scale({WORDMARK.scaleX} {WORDMARK.scaleY})" /></svg
    >

    <div class="welcome" data-glass-text>
      <p>{m.onb_tagline()}</p>
      <Button variant="primary" size="large" shape="capsule" onclick={begin}
        >{m.onb_begin()}<KeyHint keys={["↵"]} /></Button
      >
    </div>

    <div
      class="rig"
      style:transform={frame(shot)}
      style:--reach={`${shot.fade ?? 1200}px`}
      data-masked={masked}
    >
      <Window
        mode={step === "work" ? "work" : "browse"}
        {sites}
        {imported}
        playing={step === "work"}
        bind:element={windowElement}
      />
    </div>

    {#key step}
      {@const heading = headings[step]}
      {#if heading}
        <header class="head" data-glass-text>
          <h1>{heading.title()}</h1>
          {#if heading.body}<p>{heading.body()}</p>{/if}
          {#if step === "ready"}
            <ul class="features">
              {#each features as feature, index (index)}
                <li style:--i={index}><Icon icon={feature.icon} size={16} />{feature.label()}</li>
              {/each}
            </ul>
          {/if}
        </header>
      {/if}
      <div class="scene" data-scene={step}>
        {#if step === "you"}
          <You bind:name onsubmit={advance} />
        {:else if step === "import"}
          <Import />
        {:else if step === "essentials"}
          <Essentials {kept} {landing} flights={() => flights} onfailed={failed} />
        {:else if step === "launcher"}
          <Launcher />
        {/if}
      </div>
    {/key}

    <nav class="bar" aria-label={m.onb_label()} data-glass-text>
      <span class="side">
        {#if step !== "you"}<Button
            variant="ghost"
            size="large"
            shape="capsule"
            onclick={() => go(neighbour(step, -1))}>{m.onb_back()}</Button
          >{/if}
      </span>
      <span class="middle">
        {#if notice}<span class="notice" role="status">{notice}</span>{:else}
          <ol aria-label={m.onb_progress({ current: place.at, total: place.of })}>
            {#each STEPS.slice(1) as candidate, index (candidate)}
              <li
                data-state={index + 1 < place.at ? "done" : index + 1 === place.at ? "now" : ""}
              ></li>
            {/each}
          </ol>
        {/if}
      </span>
      <span class="side end">
        <Button variant="primary" size="large" shape="capsule" onclick={advance} pending={leaving}
          >{primary}<KeyHint keys={["↵"]} /></Button
        >
      </span>
    </nav>
  </div>

  <div class="top" data-tauri-drag-region>
    {#if step !== "welcome" && step !== "ready"}
      <button type="button" class="skip" onclick={() => void finish()}>{m.onb_skip()}</button>
    {/if}
    {#if !IS_MAC && windowControls}{@render windowControls()}{/if}
  </div>

  <!-- Marks thrown into the window's dock fly above the stage. -->
  <div class="flights" bind:this={flights} aria-hidden="true"></div>
</div>

<style>
  .onboarding {
    --camera: 1400ms cubic-bezier(0.7, 0, 0.12, 1);

    position: fixed;
    inset: 0;
    overflow: hidden;
    color: var(--color-text);
    font-family: var(--font-sans);
  }

  /* Nothing drawn: the window's material, or its canvas, is the ground. */
  .ground {
    position: absolute;
    inset: 0;
  }

  .stage {
    position: absolute;
    inset-block-start: 50%;
    inset-inline-start: 50%;
    inline-size: 1240px;
    block-size: 780px;
    transform-origin: center;
  }

  .air {
    position: absolute;
    inset: 0;
    inline-size: 1240px;
    block-size: 780px;
    pointer-events: none;
  }

  /* The name, solid, takes the place of the air that formed it. */
  .name {
    position: absolute;
    overflow: visible;
    opacity: 0;
    transition: opacity 1200ms var(--ease-out);
  }

  .name path {
    fill: var(--color-text);
  }

  .onboarding:is([data-phase="formed"], [data-phase="still"]) .name {
    opacity: 1;
  }

  .onboarding:not([data-step="welcome"]) .name {
    opacity: 0;
    transition-duration: 360ms;
  }

  .welcome {
    position: absolute;
    inset-block-start: 380px;
    inset-inline: 0;
    display: grid;
    justify-items: center;
    gap: 30px;
    pointer-events: none;
  }

  .welcome p {
    max-inline-size: 30ch;
    margin: 0;
    color: var(--color-muted);
    font-size: 17px;
    line-height: 1.5;
    text-align: center;
    text-wrap: balance;
  }

  .welcome > :global(*) {
    opacity: 0;
    transform: translateY(8px);
    transition:
      opacity 1000ms var(--ease-out),
      transform 1000ms var(--ease-emphasized);
  }

  .welcome > :global(:last-child) {
    transition-delay: 220ms;
  }

  .welcome :global(.ui-button) {
    gap: 12px;
    padding-inline: 20px 11px;
  }

  .onboarding[data-step="welcome"]:is([data-phase="formed"], [data-phase="still"]) .welcome {
    pointer-events: auto;
  }

  .onboarding[data-step="welcome"]:is([data-phase="formed"], [data-phase="still"])
    .welcome
    > :global(*) {
    opacity: 1;
    transform: none;
  }

  .onboarding:not([data-step="welcome"]) .welcome > :global(*) {
    transition-duration: 300ms;
    transition-delay: 0ms;
  }

  /* The window, framed for each scene by moving and scaling it, never by
     changing its layout. */
  .rig {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    inline-size: 880px;
    block-size: 560px;
    transition:
      transform var(--camera),
      --reach var(--camera),
      opacity 900ms var(--ease-out);
  }

  /* While it waits below the opening scenes, it fades into the ground. */
  .rig[data-masked="true"] {
    mask-image: linear-gradient(
      to bottom,
      black calc(var(--reach) - 180px),
      transparent var(--reach)
    );
  }

  .onboarding[data-step="welcome"]:is([data-phase="waiting"], [data-phase="forming"]) .rig {
    opacity: 0;
  }

  .onboarding[data-step="welcome"]:is([data-phase="formed"], [data-phase="still"]) .rig {
    transition-delay: 500ms;
  }

  @property --reach {
    syntax: "<length>";
    inherits: false;
    initial-value: 1200px;
  }

  .head {
    position: absolute;
    inset-block-start: 74px;
    inset-inline: 0;
    display: grid;
    justify-items: center;
    gap: 12px;
    text-align: center;
    pointer-events: none;
  }

  .head h1 {
    margin: 0;
    font-size: 40px;
    font-weight: 500;
    line-height: 1.1;
    letter-spacing: -0.032em;
  }

  .head p {
    max-inline-size: 52ch;
    margin: 0;
    color: var(--color-muted);
    font-size: 15px;
    line-height: 1.5;
    text-wrap: balance;
  }

  .head > * {
    animation: rise 900ms var(--ease-emphasized) 250ms backwards;
  }

  .head > p {
    animation-delay: 360ms;
  }

  .features {
    display: flex;
    gap: 30px;
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
    animation: none;
  }

  .features li {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--color-muted);
    font-size: 14px;
    font-weight: 500;
    white-space: nowrap;
    animation: rise 800ms var(--ease-emphasized) backwards;
    animation-delay: calc(380ms + var(--i) * 70ms);
  }

  .features li :global(svg) {
    color: var(--color-text);
  }

  .scene {
    position: absolute;
    inset-inline: 0;
    display: grid;
    justify-items: center;
    animation: rise 900ms var(--ease-emphasized) 420ms backwards;
  }

  .scene[data-scene="you"] {
    inset-block-start: 236px;
  }

  /* Beside the window, centred on it however many browsers were found. */
  .scene[data-scene="import"] {
    inset-block-start: 272px;
    inset-inline: 856px auto;
    block-size: 392px;
    align-content: center;
  }

  .scene[data-scene="essentials"] {
    inset-block-start: 236px;
    inset-inline: 142px auto;
  }

  .scene[data-scene="launcher"] {
    inset-block-start: 226px;
  }

  /* One quiet row for the whole journey, at the foot of the stage: nothing
     floats over the scene it steers. */
  .bar {
    position: absolute;
    inset-block-end: 34px;
    inset-inline: 56px;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    block-size: 44px;
    transition:
      opacity 600ms var(--ease-out),
      transform 600ms var(--ease-emphasized);
  }

  .onboarding:is([data-step="welcome"], [data-leaving="true"]) .bar {
    opacity: 0;
    pointer-events: none;
    transform: translateY(12px);
  }

  .side {
    display: flex;
  }

  .side.end {
    justify-content: flex-end;
  }

  .side.end :global(.ui-button) {
    gap: 12px;
    min-inline-size: 132px;
    padding-inline: 20px 12px;
  }

  /* On the lit fill a control-grey key vanishes; it takes the label's ink. */
  .onboarding :global(.ui-button[data-variant="primary"] .ui-keys kbd) {
    background: color-mix(in srgb, var(--color-on-lit) 10%, transparent);
    color: color-mix(in srgb, var(--color-on-lit) 64%, transparent);
  }

  .middle ol {
    display: flex;
    gap: 5px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .middle li {
    inline-size: 18px;
    block-size: 3px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill-strong);
    transition:
      inline-size var(--motion-page) var(--ease-emphasized),
      background-color var(--motion-slow) var(--ease-out);
  }

  .middle li[data-state="done"] {
    background: var(--color-faint);
  }

  .middle li[data-state="now"] {
    inline-size: 34px;
    background: var(--color-text);
  }

  .notice {
    color: var(--color-danger);
    font-size: 12.5px;
  }

  .top {
    position: absolute;
    inset-block-start: 0;
    inset-inline: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    block-size: 52px;
    padding-inline-end: 20px;
  }

  .skip {
    padding: 6px 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: var(--color-faint);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
    transition:
      color var(--motion-instant) var(--ease-smooth),
      background-color var(--motion-instant) var(--ease-smooth);
  }

  .skip:hover {
    background: var(--color-fill);
    color: var(--color-muted);
  }

  /* Leaving: the words go first, then the whole scene steps back and away
     while the browser fades in beneath it. Onboarding draws a miniature,
     so it never pretends to become the real window. */
  .onboarding[data-leaving="true"] .stage {
    opacity: 0;
    scale: 0.9;
    transition:
      opacity 820ms var(--ease-out) 120ms,
      scale 1000ms var(--ease-emphasized);
  }

  .onboarding[data-leaving="true"] :is(.head, .scene, .top) {
    opacity: 0;
    transition: opacity 360ms var(--ease-exit);
  }

  .flights {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }

  .flights :global(.flight) {
    position: fixed;
    object-fit: contain;
    will-change: transform, opacity;
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(10px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .onboarding[data-leaving="true"] .stage,
    .rig,
    .bar,
    .name,
    .welcome > :global(*) {
      transition: none;
    }

    .head > *,
    .features li,
    .scene {
      animation: none;
    }
  }
</style>
