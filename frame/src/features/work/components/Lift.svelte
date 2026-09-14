<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import Icon from "$shared/ui/Icon";
  import { Cancel01Icon } from "../lib/icons";
  import * as m from "$shared/i18n/messages";
  let {
    origin,
    bounds,
    preferred = { width: 720, height: 520 },
    title,
    onclose,
    children,
  }: {
    origin: DOMRect | null;
    bounds: DOMRect;
    preferred?: { width: number; height: number };
    title: string;
    onclose: () => void;
    children: Snippet;
  } = $props();
  const width = $derived(Math.max(280, Math.min(preferred.width, bounds.width - 64)));
  const height = $derived(Math.max(200, Math.min(preferred.height, bounds.height - 64)));
  const left = $derived(bounds.left + (bounds.width - width) / 2);
  const top = $derived(bounds.top + (bounds.height - height) / 2);
  let panel = $state<HTMLElement>();
  let backdrop = $state<HTMLElement>();
  let closing = false;
  const reduce = () =>
    window.matchMedia("(prefers-reduced-motion: reduce)").matches ||
    document.documentElement.dataset.reduceMotion === "true";
  function keyframes(): Record<string, string | number>[] {
    if (!origin)
      return [
        { opacity: 0, transform: "scale(0.96)" },
        { opacity: 1, transform: "none" },
      ];
    const sx = origin.width / width;
    const sy = origin.height / height;
    const dx = origin.left - left;
    const dy = origin.top - top;
    return [
      { transform: `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})`, opacity: 0.6 },
      { transform: "none", opacity: 1 },
    ];
  }
  onMount(() => {
    const element = panel;
    const dim = backdrop;
    if (!element) return;
    element.focus({ preventScroll: true });
    if (reduce()) return;
    try {
      element.animate(keyframes(), { duration: 280, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" });
      dim?.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 200 });
    } catch {
      /* animation is decorative */
    }
  });
  export function close() {
    if (closing) return;
    closing = true;
    const element = panel;
    const dim = backdrop;
    if (!element || reduce()) {
      onclose();
      return;
    }
    const frames = keyframes().reverse();
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      onclose();
    };
    setTimeout(finish, 300);
    try {
      dim?.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 180, fill: "forwards" });
      element
        .animate(frames, {
          duration: 220,
          easing: "cubic-bezier(0.4, 0, 1, 1)",
          fill: "forwards",
        })
        .finished.then(finish, finish);
    } catch {
      finish();
    }
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  }}
/>
<div
  class="backdrop"
  role="presentation"
  bind:this={backdrop}
  style:inset={`${bounds.top}px auto auto ${bounds.left}px`}
  style:inline-size={`${bounds.width}px`}
  style:block-size={`${bounds.height}px`}
  onpointerdown={(event) => {
    if (event.target === event.currentTarget) close();
  }}
></div>
<div
  class="lift"
  bind:this={panel}
  tabindex="-1"
  role="dialog"
  aria-modal="true"
  aria-label={title}
  style:left={`${left}px`}
  style:top={`${top}px`}
  style:inline-size={`${width}px`}
  style:block-size={`${height}px`}
>
  <button type="button" class="close" aria-label={m.work_env_lift_close()} onclick={close}>
    <Icon icon={Cancel01Icon} size={14} />
  </button>
  <div class="content">{@render children()}</div>
</div>

<style>
  .backdrop {
    position: fixed;
    z-index: 25;
    border-radius: var(--content-radius);
    background: color-mix(in srgb, var(--color-canvas) 55%, transparent);
  }

  .lift {
    position: fixed;
    z-index: 26;
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      var(--shadow-overlay);
    color: var(--color-text);
    transform-origin: top left;
    outline: none;
    overflow: hidden;
  }

  .close {
    position: absolute;
    inset-block-start: 10px;
    inset-inline-end: 10px;
    z-index: 2;
    display: grid;
    place-items: center;
    inline-size: 26px;
    block-size: 26px;
    border: 0;
    border-radius: 50%;
    background: var(--color-fill);
    color: var(--color-muted);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      color var(--motion-fast) var(--ease-smooth);
  }

  .close:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .content {
    flex: 1;
    min-block-size: 0;
    padding: 20px 24px;
    overflow: auto;
  }
</style>
