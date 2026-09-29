<script lang="ts">
  /**
   * Where a run stands, as one small ring: an arc turning while it works (the
   * arc a loading tab's mark turns), the accent with a centre while it waits
   * on the person, a tick once done, a rest once stopped. Only the working arc
   * moves, by transform alone, and only while it works.
   */
  let {
    state: mark = "idle",
    size = 16,
  }: { state?: "live" | "waiting" | "done" | "stopped" | "idle"; size?: number } = $props();
</script>

<svg class="run-mark {mark}" width={size} height={size} viewBox="0 0 16 16" aria-hidden="true">
  <circle class="track" cx="8" cy="8" r="6.25" />
  {#if mark === "live"}<g class="orbit"
      ><path class="arc" d="M 8 1.75 A 6.25 6.25 0 0 1 14.25 8" /></g
    >
  {:else if mark === "waiting"}<circle class="centre" cx="8" cy="8" r="2.75" />
  {:else if mark === "done"}<path class="tick" d="M 5.25 8.25 L 7.2 10.1 L 10.9 6" />
  {:else if mark === "stopped"}<path class="rest" d="M 5.5 8 L 10.5 8" />{/if}
</svg>

<style>
  .run-mark {
    display: block;
    flex: none;
    overflow: visible;
  }

  .track,
  .arc,
  .tick,
  .rest {
    fill: none;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .track {
    stroke: var(--color-border-strong);
    transition: stroke var(--motion-base) var(--ease-out);
  }

  .arc {
    stroke: var(--color-text);
  }

  .orbit {
    transform-origin: 8px 8px;
    animation: run-orbit 1.4s linear infinite;
  }

  .waiting .track {
    stroke: var(--color-accent);
  }

  .centre {
    fill: var(--color-accent);
  }

  .tick,
  .rest {
    stroke: var(--color-muted);
  }

  @keyframes run-orbit {
    to {
      rotate: 1turn;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .orbit {
      animation: none;
    }
  }
</style>
