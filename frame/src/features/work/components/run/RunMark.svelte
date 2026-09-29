<script lang="ts">
  import { Orb } from "$shared/ui/presence";
  /**
   * Where a run stands: a small dot sphere turning while it works, a ring with
   * a centre while it waits on the person, a tick once done, a rest once
   * stopped. Only the working sphere moves, and only while it works.
   */
  let {
    state: mark = "idle",
    size = 16,
  }: { state?: "live" | "waiting" | "done" | "stopped" | "idle"; size?: number } = $props();
</script>

{#if mark === "live"}<Orb kind="working" {size} />{:else}<svg
    class="run-mark {mark}"
    width={size}
    height={size}
    viewBox="0 0 16 16"
    aria-hidden="true"
  >
    <circle class="track" cx="8" cy="8" r="6.25" />
    {#if mark === "waiting"}<circle class="centre" cx="8" cy="8" r="2.75" />
    {:else if mark === "done"}<path class="tick" d="M 5.25 8.25 L 7.2 10.1 L 10.9 6" />
    {:else if mark === "stopped"}<path class="rest" d="M 5.5 8 L 10.5 8" />{/if}
  </svg>{/if}

<style>
  .run-mark {
    display: block;
    flex: none;
    overflow: visible;
  }

  .track,
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
</style>
