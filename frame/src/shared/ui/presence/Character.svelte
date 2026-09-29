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
  import { characterMask, type CharacterKind } from "./shapes";
  import { watchStill } from "./still";

  let {
    kind = "lead",
    mood = "rest",
    size = 20,
    label,
    grounded = false,
  }: {
    kind?: CharacterKind;
    mood?: Mood;
    size?: number;
    /** Said to assistive technology; without it the character is decoration beside its name. */
    label?: string;
    /** Standing on the canvas: a soft contact shadow under it. */
    grounded?: boolean;
  } = $props();

  const live = $derived(LIVE.has(mood));
  const mask = $derived(characterMask(kind));
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
      <span class="body" style:mask-image={mask}></span>
      <span class="face">
        {#key mood}<span class="eyes"><i class="eye"></i><i class="eye"></i></span>{/key}
      </span>
    </span>
  </span>
</span>

<style>
  .character {
    --hue: var(--color-agent-lead);
    --eye-w: 0.088em;
    --eye-h: 0.15em;
    --eye-gap: 0.13em;
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
    --hue: var(--color-agent-browser);
    --blink-delay: 2.3s;
  }

  .research {
    --hue: var(--color-agent-research);
    --blink-delay: 0.4s;
  }

  .computer {
    --hue: var(--color-agent-computer);
    --blink-delay: 3.1s;
    --eye-w: 0.1em;
    --eye-h: 0.12em;
  }

  .connection {
    --hue: var(--color-agent-connection);
    --blink-delay: 1.7s;
  }

  .lead {
    --eye-w: 0.094em;
    --eye-h: 0.16em;
  }

  .pose,
  .figure,
  .body,
  .face {
    position: absolute;
    inset: 0;
  }

  .pose {
    transition: transform var(--motion-slow) var(--ease-spring);
  }

  /* Lit from the upper left: a soft highlight, the hue's body falling into its
     own deep tone, and the light the ground throws back along the lower rim. */
  .body {
    background:
      radial-gradient(
        30% 22% at 35% 24%,
        color-mix(in oklab, var(--color-agent-light) 82%, transparent),
        transparent
      ),
      radial-gradient(
        64% 34% at 52% 96%,
        color-mix(
          in oklab,
          color-mix(in oklab, var(--hue) 60%, var(--color-agent-light)) 70%,
          transparent
        ),
        transparent
      ),
      radial-gradient(
        118% 118% at 32% 24%,
        color-mix(in oklab, var(--hue) 58%, var(--color-agent-light)) 0%,
        var(--hue) 40%,
        color-mix(in oklab, var(--hue) 52%, var(--color-agent-deep)) 94%
      );
    mask-size: 100% 100%;
    mask-repeat: no-repeat;
  }

  /* The lead is a pearl: the same light, with a cool sheen turning round its lower side. */
  .lead .body {
    border-radius: 50%;
    background:
      radial-gradient(
        30% 22% at 35% 24%,
        color-mix(in oklab, var(--color-agent-light) 92%, transparent),
        transparent
      ),
      radial-gradient(
        70% 52% at 74% 80%,
        color-mix(in oklab, var(--color-agent-lead-sheen) 72%, transparent),
        transparent
      ),
      radial-gradient(
        118% 118% at 32% 24%,
        var(--color-agent-light) 0%,
        var(--hue) 38%,
        color-mix(in oklab, var(--hue) 50%, var(--color-agent-deep)) 96%
      );
  }

  .eyes {
    position: absolute;
    inset-block-start: 45%;
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
    background: var(--color-agent-eye);
  }

  .computer .eye {
    border-radius: 0.025em;
  }

  /* Waiting on you: eyes a little wider, on you, the head tilted. */
  .waiting .pose {
    transform: rotate(-9deg);
  }

  .waiting .eye {
    block-size: calc(var(--eye-h) * 1.14);
    inline-size: calc(var(--eye-w) * 1.1);
  }

  /* Done: content, eyes closed in two small arcs. */
  .done .eye {
    inline-size: calc(var(--eye-w) * 1.7);
    block-size: calc(var(--eye-w) * 0.95);
    border: 0.028em solid var(--color-agent-eye);
    border-block-end: 0;
    border-radius: 50% 50% 0 0 / 100% 100% 0 0;
    background: transparent;
  }

  /* Stopped: eyes at rest in two short lines, the head lowered, the colour gone quiet. */
  .stopped .pose {
    transform: translateY(0.03em) rotate(7deg);
  }

  .stopped .eye {
    inline-size: calc(var(--eye-w) * 1.5);
    block-size: 0.026em;
  }

  .stopped .body {
    filter: saturate(0.3);
    opacity: 0.78;
  }

  .ground {
    position: absolute;
    inset-inline: 14%;
    inset-block-end: -10%;
    block-size: 18%;
    border-radius: 50%;
    background: radial-gradient(closest-side, var(--color-agent-ground), transparent);
  }

  .fine .eye::after {
    content: "";
    display: block;
    inline-size: 38%;
    block-size: 24%;
    margin: 14% 0 0 44%;
    border-radius: 50%;
    background: color-mix(in oklab, var(--color-agent-light) 70%, transparent);
  }

  .fine.done .eye::after,
  .fine.stopped .eye::after {
    content: none;
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

  .working .face {
    transform: translate(0.028em, 0.022em);
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
      transform: translate(0.034em, -0.03em);
    }

    34%,
    46% {
      transform: translate(-0.03em, -0.036em);
    }

    62%,
    84% {
      transform: translate(0.034em, -0.03em);
    }
  }

  @keyframes read {
    0% {
      transform: translate(-0.04em, 0.024em);
    }

    82% {
      transform: translate(0.04em, 0.024em);
    }

    100% {
      transform: translate(-0.04em, 0.024em);
    }
  }

  @keyframes search {
    0%,
    20% {
      transform: translate(-0.046em, -0.004em);
    }

    26%,
    48% {
      transform: translate(0.046em, -0.004em);
    }

    54%,
    70% {
      transform: translate(0.01em, -0.03em);
    }

    76%,
    100% {
      transform: translate(-0.046em, -0.004em);
    }
  }

  @keyframes hop {
    40% {
      transform: translateY(-0.12em);
    }
  }
</style>
