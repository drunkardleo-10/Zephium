<script lang="ts" module>
  export type Mood =
    "rest" | "thinking" | "reading" | "searching" | "working" | "waiting" | "done" | "stopped";

  const LIVE: ReadonlySet<Mood> = new Set([
    "thinking",
    "reading",
    "searching",
    "working",
    "waiting",
  ]);
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { characterMask, leadLookHere, type CharacterKind, type LeadLook } from "./shapes";
  import { watchStill } from "./still";

  let {
    kind = "lead",
    mood = "rest",
    size = 20,
    label,
    grounded = false,
    look,
  }: {
    kind?: CharacterKind;
    mood?: Mood;
    size?: number;
    /** Said to assistive technology; without it the character is decoration beside its name. */
    label?: string;
    /** Standing on the canvas: a soft contact shadow under it. */
    grounded?: boolean;
    /** The lead's figure; without it, the one the enclosing work gave its lead. */
    look?: LeadLook;
  } = $props();

  const given = leadLookHere();
  const figure = $derived<LeadLook>(look ?? given?.() ?? "pearl");
  const live = $derived(LIVE.has(mood));
  const mask = $derived(characterMask(kind, figure));
  let host = $state<HTMLElement>();
  let still = $state(true);
  /** Expressions change behind a blink, never on arrival; a finish cheers only when seen happening. */
  let turned = $state(false);
  let cheer = $state(false);
  let before: Mood | undefined;
  $effect(() => {
    const now = mood;
    if (before !== undefined && before !== now) turned = true;
    const was = before;
    cheer = now === "done" && untrack(() => cheer || (was !== undefined && LIVE.has(was)));
    before = now;
  });

  $effect(() => {
    const element = host;
    if (!element || !live) return;
    return watchStill(element, (next) => (still = next));
  });
</script>

<span
  class="character {kind} {mood}"
  class:live
  class:grounded
  class:fine={size >= 28}
  class:turned
  class:cheer
  data-look={kind === "lead" ? figure : undefined}
  data-still={live && still ? "" : undefined}
  bind:this={host}
  style:--size="{size}px"
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
>
  {#if grounded}<span class="ground"></span>{/if}
  <span class="pose">
    <span class="figure">
      {#if kind === "lead"}<span class="orbit back"><i></i></span>{/if}
      <span class="body" style:mask-image={mask}></span>
      <span class="visor">
        <span class="face">
          {#key mood}<span class="eyes"><i class="eye"></i><i class="eye"></i></span>{/key}
        </span>
      </span>
      {#if kind === "lead"}<span class="orbit front"><i></i></span>{/if}
    </span>
  </span>
</span>

<style>
  .character {
    --hue: var(--color-agent-browser);
    --hue-to: var(--hue);
    --visor-w: 0.56em;
    --visor-h: 0.25em;
    --visor-y: 47%;
    --eye-w: 0.078em;
    --eye-h: 0.128em;
    --eye-gap: 0.11em;
    --blink-delay: 1.2s;

    position: relative;
    display: block;
    flex: none;
    inline-size: var(--size);
    block-size: var(--size);
    font-size: var(--size);
    line-height: 0;
    contain: layout style;
  }

  .browser {
    --blink-delay: 2.3s;
  }

  .research {
    --hue: var(--color-agent-research);
    --blink-delay: 0.4s;
    --visor-w: 0.56em;
  }

  .computer {
    --hue: var(--color-agent-computer);
    --blink-delay: 3.1s;
    --visor-w: 0.66em;
    --visor-y: 49%;
  }

  .connection {
    --hue: var(--color-agent-connection);
    --blink-delay: 1.7s;
    --visor-w: 0.5em;
    --visor-y: 49%;
  }

  /* The lead: a two-tone figure from its family, with an orbit round it. */
  .lead {
    --hue: var(--color-lead-pearl);
    --hue-to: var(--color-lead-pearl-to);
    --visor-w: 0.62em;
    --visor-y: 48%;
  }

  [data-look="orb"] {
    --hue: var(--color-lead-orb);
    --hue-to: var(--color-lead-orb-to);
  }

  [data-look="drop"] {
    --hue: var(--color-lead-drop);
    --hue-to: var(--color-lead-drop-to);
    --visor-w: 0.56em;
    --visor-y: 62%;
  }

  [data-look="prism"] {
    --hue: var(--color-lead-prism);
    --hue-to: var(--color-lead-prism-to);
    --visor-w: 0.5em;
    --visor-y: 64%;
  }

  [data-look="gem"] {
    --hue: var(--color-lead-gem);
    --hue-to: var(--color-lead-gem-to);
    --visor-w: 0.54em;
    --visor-y: 50%;
  }

  [data-look="egg"] {
    --hue: var(--color-lead-egg);
    --hue-to: var(--color-lead-egg-to);
    --visor-y: 55%;
  }

  .pose,
  .figure,
  .body {
    position: absolute;
    inset: 0;
  }

  .pose {
    transition: transform var(--motion-slow) var(--ease-spring);
  }

  /* Lit from the upper left: a soft highlight, the hue running into its second
     tone, and the light the ground throws back along the lower rim. */
  .body {
    background:
      radial-gradient(
        28% 20% at 34% 22%,
        color-mix(in oklab, var(--color-agent-light) 80%, transparent),
        transparent
      ),
      radial-gradient(
        64% 34% at 52% 98%,
        color-mix(
          in oklab,
          color-mix(in oklab, var(--hue-to) 55%, var(--color-agent-light)) 70%,
          transparent
        ),
        transparent
      ),
      linear-gradient(
        150deg,
        color-mix(in oklab, var(--hue) 62%, var(--color-agent-light)) 0%,
        var(--hue) 34%,
        var(--hue-to) 72%,
        color-mix(in oklab, var(--hue-to) 55%, var(--color-agent-deep)) 100%
      );
    mask-size: 100% 100%;
    mask-repeat: no-repeat;
  }

  /* Dark glass across the face, a line of light along its top, the eyes lit inside it. */
  .visor {
    position: absolute;
    inset-block-start: var(--visor-y);
    inset-inline-start: 50%;
    inline-size: var(--visor-w);
    block-size: var(--visor-h);
    overflow: hidden;
    border: max(0.5px, 0.018em) solid var(--color-visor-rim);
    border-radius: var(--radius-capsule);
    background:
      linear-gradient(
        180deg,
        color-mix(in oklab, var(--color-agent-light) 16%, transparent) 0%,
        transparent 38%
      ),
      radial-gradient(
        70% 90% at 50% 60%,
        color-mix(in oklab, var(--hue) 34%, var(--color-visor)),
        var(--color-visor)
      );
    translate: -50% -50%;
  }

  .face {
    position: absolute;
    inset: 0;
  }

  .eyes {
    position: absolute;
    inset-block-start: 52%;
    inset-inline-start: 50%;
    display: flex;
    gap: var(--eye-gap);
    translate: -50% -50%;
  }

  .turned .eyes {
    animation: eyes-open var(--motion-fast) var(--ease-out);
  }

  .eye {
    display: block;
    inline-size: var(--eye-w);
    block-size: var(--eye-h);
    border-radius: var(--radius-capsule);
    background: var(--color-eye-lit);
  }

  .computer .eye {
    border-radius: 0.02em;
  }

  /* Past a small size the eyes carry a halo of their own hue. */
  .fine .eye {
    background: radial-gradient(
      closest-side,
      var(--color-eye-lit) 62%,
      color-mix(in oklab, var(--hue) 70%, var(--color-eye-lit))
    );
  }

  /* The lead's orbit: a thin ring tipped toward the viewer, half behind the
     figure and half in front of it. */
  .orbit {
    position: absolute;
    inset-inline: -12%;
    inset-block-start: 52%;
    block-size: 26%;
    rotate: -16deg;
    translate: 0 -30%;
  }

  .orbit i {
    position: absolute;
    inset: 0;
    border: max(0.75px, 0.03em) solid
      color-mix(in oklab, var(--hue-to) 55%, var(--color-agent-light));
    border-radius: 50%;
    opacity: 0.85;
  }

  .orbit.front {
    clip-path: inset(50% 0 0 0);
  }

  .orbit.back i {
    opacity: 0.4;
  }

  /* Waiting on you: eyes a little wider, on you, the head tilted. */
  .waiting .pose {
    transform: rotate(-9deg);
  }

  .waiting .eye {
    block-size: calc(var(--eye-h) * 1.2);
    inline-size: calc(var(--eye-w) * 1.15);
  }

  /* Working: narrowed, focused. */
  .working .eye {
    block-size: calc(var(--eye-h) * 0.62);
  }

  /* Done: content, eyes closed in two small lit arcs. */
  .done .eye {
    inline-size: calc(var(--eye-w) * 1.9);
    block-size: calc(var(--eye-w) * 1.05);
    border: max(0.6px, 0.024em) solid var(--color-eye-lit);
    border-block-end: 0;
    border-radius: 50% 50% 0 0 / 100% 100% 0 0;
    background: transparent;
  }

  /* Stopped: eyes at rest in two short dim lines, the head lowered, the colour gone quiet. */
  .stopped .pose {
    transform: translateY(0.03em) rotate(7deg);
  }

  .stopped .eye {
    inline-size: calc(var(--eye-w) * 1.6);
    block-size: max(0.6px, 0.022em);
    opacity: 0.55;
  }

  .stopped .body,
  .stopped .orbit {
    filter: saturate(0.25);
    opacity: 0.75;
  }

  .ground {
    position: absolute;
    inset-inline: 14%;
    inset-block-end: -10%;
    block-size: 18%;
    border-radius: 50%;
    background: radial-gradient(closest-side, var(--color-agent-ground), transparent);
  }

  /* Only a live character moves: a slow float, a glance, a blink. Waiting asks
     twice, then keeps still until the person answers. */
  .live {
    --beat: 3.4s;
    --beats: infinite;
  }

  .waiting {
    --beat: 1.6s;
    --beats: 2;
  }

  .live .figure {
    animation: float var(--beat) var(--ease-in-out) var(--beats);
  }

  .live .ground {
    animation: ground var(--beat) var(--ease-in-out) var(--beats);
  }

  .live .eyes {
    animation: blink 4.8s linear var(--blink-delay) var(--beats);
  }

  .live.turned .eyes {
    animation:
      eyes-open var(--motion-fast) var(--ease-out),
      blink 4.8s linear var(--blink-delay) var(--beats);
  }

  .thinking .face {
    animation: ponder 6.4s var(--ease-in-out) infinite;
  }

  .reading .face {
    animation: read 1.9s var(--ease-in-out) infinite;
  }

  .searching .face {
    animation: search 3.6s var(--ease-out) infinite;
  }

  .cheer .figure {
    animation: hop var(--motion-page) var(--ease-spring);
  }

  .character[data-still] .figure,
  .character[data-still] .face,
  .character[data-still] .eyes,
  .character[data-still] .ground {
    animation-play-state: paused;
  }

  @keyframes float {
    50% {
      transform: translateY(-0.045em);
    }
  }

  @keyframes ground {
    50% {
      transform: scale(0.84);
      opacity: 0.7;
    }
  }

  @keyframes blink {
    0%,
    93%,
    100% {
      transform: scaleY(1);
    }

    96% {
      transform: scaleY(0.1);
    }
  }

  @keyframes eyes-open {
    from {
      transform: scaleY(0.1);
    }
  }

  @keyframes ponder {
    0%,
    100% {
      transform: translate(0.05em, -0.02em);
    }

    34%,
    46% {
      transform: translate(-0.05em, -0.02em);
    }

    62%,
    84% {
      transform: translate(0.05em, -0.02em);
    }
  }

  @keyframes read {
    0% {
      transform: translate(-0.07em, 0.01em);
    }

    82% {
      transform: translate(0.07em, 0.01em);
    }

    100% {
      transform: translate(-0.07em, 0.01em);
    }
  }

  @keyframes search {
    0%,
    20% {
      transform: translate(-0.08em, 0);
    }

    26%,
    48% {
      transform: translate(0.08em, 0);
    }

    54%,
    70% {
      transform: translate(0.01em, -0.02em);
    }

    76%,
    100% {
      transform: translate(-0.08em, 0);
    }
  }

  @keyframes hop {
    40% {
      transform: translateY(-0.12em);
    }
  }
</style>
