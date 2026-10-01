<script lang="ts">
  import { orbKeyframes, orbStrip } from "./orb";
  import { watchStill } from "./still";

  let {
    size = 16,
    label,
  }: {
    size?: number;
    /** Said to assistive technology; without it the orb is decoration beside its words. */
    label?: string;
  } = $props();

  const strip = $derived(orbStrip(size));
  let host = $state<HTMLElement>();
  let film = $state<HTMLElement>();

  $effect(() => {
    const element = film;
    const watched = host;
    if (!element || !watched) return;
    const { frames, fps } = strip;
    const run = element.animate(orbKeyframes(frames, size), {
      duration: (frames / fps) * 1000,
      iterations: Infinity,
      easing: "linear",
    });
    run.pause();
    const stop = watchStill(watched, (still) => {
      if (still) run.pause();
      else run.play();
    });
    return () => {
      stop();
      run.cancel();
    };
  });
</script>

<span
  class="orb"
  bind:this={host}
  style:--size="{size}px"
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
>
  <span
    class="film"
    bind:this={film}
    style:inline-size="{size * strip.frames}px"
    style:mask-image={strip.mask}
  ></span>
</span>

<style>
  .orb {
    position: relative;
    display: block;
    flex: none;
    inline-size: var(--size);
    block-size: var(--size);
    overflow: hidden;
    contain: strict;
  }

  .film {
    position: absolute;
    inset-block: 0;
    inset-inline-start: 0;
    background: var(--orb-tone, var(--color-dots));
    mask-size: 100% 100%;
    mask-repeat: no-repeat;
  }
</style>
