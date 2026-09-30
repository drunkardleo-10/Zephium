<script lang="ts">
  import { watchStill } from "./still";

  /**
   * Words at work: a band of light passes along them. The band is a masked
   * window sliding one way while its bright copy of the words slides back, so
   * the light moves and the words stay put, by transform alone.
   */
  let { text, running = true }: { text: string; running?: boolean } = $props();

  let host = $state<HTMLElement>();
  let still = $state(true);
  $effect(() => {
    const element = host;
    if (!element || !running) return;
    return watchStill(element, (next) => (still = next));
  });
</script>

<span class="shimmer" class:running data-still={running && still ? "" : undefined} bind:this={host}
  ><span class="words">{text}</span>{#if running}<span class="band" aria-hidden="true"
      ><span class="lit">{text}</span></span
    >{/if}</span
>

<style>
  .shimmer {
    position: relative;
    display: inline-block;
    max-inline-size: 100%;
    overflow: hidden;
    vertical-align: bottom;
    white-space: nowrap;
  }

  .words {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .running .words {
    color: var(--shimmer-rest, var(--color-muted));
  }

  .band {
    position: absolute;
    inset-block: 0;
    inset-inline-start: 0;
    inline-size: 40%;
    overflow: hidden;
    mask-image: linear-gradient(90deg, transparent, black 45%, black 55%, transparent);
    animation: band 2.4s linear infinite;
  }

  .lit {
    position: absolute;
    inset-block: 0;
    inset-inline-start: 0;
    display: block;
    inline-size: 250%;
    overflow: hidden;
    color: var(--shimmer-lit, var(--color-text));
    text-overflow: ellipsis;
    animation: lit 2.4s linear infinite;
  }

  [data-still] .band,
  [data-still] .lit {
    animation-play-state: paused;
  }

  [data-still] .band {
    visibility: hidden;
  }

  @keyframes band {
    0% {
      transform: translateX(-100%);
    }

    72%,
    100% {
      transform: translateX(250%);
    }
  }

  @keyframes lit {
    0% {
      transform: translateX(40%);
    }

    72%,
    100% {
      transform: translateX(-100%);
    }
  }
</style>
