<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import { duration, easing, reducedMotion } from "$shared/lib/motion";
  let {
    label,
    side = "right",
    onclose,
    children,
  }: {
    label: string;
    /** Which side of its anchor it opens on. */
    side?: "right" | "below";
    onclose: () => void;
    children: Snippet;
  } = $props();
  let root = $state<HTMLElement>();
  onMount(() => {
    root?.animate(
      [
        { opacity: 0, transform: side === "right" ? "translateX(-4px)" : "translateY(-4px)" },
        { opacity: 1, transform: "none" },
      ],
      { duration: duration(reducedMotion() ? "instant" : "fast"), easing: easing("out") },
    );
    root?.focus({ preventScroll: true });
    // Anything pressed outside it closes it; the press still does what it does.
    const away = (event: PointerEvent) => {
      if (root && !root.contains(event.target as Node)) onclose();
    };
    window.addEventListener("pointerdown", away, true);
    return () => window.removeEventListener("pointerdown", away, true);
  });
</script>

<div
  bind:this={root}
  class="popover nodrag nopan nowheel {side}"
  role="dialog"
  aria-label={label}
  tabindex="-1"
  onkeydown={(event) => {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    onclose();
  }}
>
  {@render children()}
</div>

<style>
  /* Anchored beside what opened it, above its neighbours; never centred over the canvas. */
  .popover {
    position: absolute;
    z-index: 5;
    inline-size: 288px;
    padding: 14px;
    border-radius: var(--radius-control-large);
    background: var(--color-float);
    box-shadow: var(--shadow-popover);
    color: var(--color-text);
    font-size: var(--text-body);
    cursor: default;
  }

  .popover:focus {
    outline: none;
  }

  .right {
    inset-block-start: 0;
    inset-inline-start: calc(100% + 10px);
  }

  .below {
    inset-block-start: calc(100% + 8px);
    inset-inline-start: 0;
  }
</style>
