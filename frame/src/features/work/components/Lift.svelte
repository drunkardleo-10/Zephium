<script lang="ts">
  import { onMount, tick, type Snippet } from "svelte";
  import Icon from "$shared/ui/Icon";
  import { duration, easing, reducedMotion } from "$shared/lib/motion";
  import { provideSiteMarks } from "$shared/ui/data/Artifact/site-marks";
  import { siteMark } from "./cards/HostGlyph.svelte";
  import { Cancel01Icon } from "../lib/icons";
  import * as m from "$shared/i18n/messages";
  let {
    origin,
    source = null,
    bounds,
    preferred = { width: 720, height: 520 },
    title,
    onclose,
    children,
  }: {
    /** The card's rect when it was opened; it finds its card when no id is given. */
    origin: DOMRect | null;
    /** The canvas item the lift grows out of, found by the `data-card-id` its card carries. */
    source?: string | null;
    bounds: DOMRect;
    preferred?: { width: number; height: number };
    title: string;
    onclose: () => void;
    children: Snippet;
  } = $props();
  provideSiteMarks(siteMark);
  /** Never closer than 48 px to the canvas's edges, whatever the kind asks for. */
  const MARGIN = 48;
  const width = $derived(Math.max(280, Math.min(preferred.width, bounds.width - MARGIN * 2)));
  const height = $derived(Math.max(200, Math.min(preferred.height, bounds.height - MARGIN * 2)));
  const left = $derived(bounds.left + (bounds.width - width) / 2);
  const top = $derived(bounds.top + (bounds.height - height) / 2);
  let panel = $state<HTMLElement>();
  let face = $state<HTMLElement>();
  let backdrop = $state<HTMLElement>();
  /** The header keeps a hairline under it once the well has scrolled. */
  let scrolled = $state(false);
  /** Hidden until the morph's first frame has been taken from the card. */
  let shown = $state(false);
  let closing = false;
  let card: HTMLElement | null = null;
  let ghost: HTMLElement | null = null;
  const MORPH = "work-lift-morph";
  const RETURN = "work-lift-return";

  /** The card this lift stands for: by id, or the one sitting where it was opened. */
  function findCard(): HTMLElement | null {
    const cards = [...document.querySelectorAll<HTMLElement>("[data-card-id]")];
    if (source) return cards.find((candidate) => candidate.dataset.cardId === source) ?? null;
    if (!origin) return null;
    let best: HTMLElement | null = null;
    let distance = 8;
    for (const candidate of cards) {
      const rect = candidate.getBoundingClientRect();
      const off =
        Math.abs(rect.left - origin.left) +
        Math.abs(rect.top - origin.top) +
        Math.abs(rect.width - origin.width) +
        Math.abs(rect.height - origin.height);
      if (off < distance) [best, distance] = [candidate, off];
    }
    return best;
  }

  const morphs = () => typeof document.startViewTransition === "function";
  /** A frame a lift's box takes to sit exactly on a rect. */
  const onto = (rect: DOMRect) =>
    `translate(${rect.left - left}px, ${rect.top - top}px) scale(${rect.width / width}, ${rect.height / height})`;
  /** A copy of the card at a rect, drawn at its own layout size and scaled there. */
  function copy(of: HTMLElement): HTMLElement {
    const clone = of.cloneNode(true) as HTMLElement;
    clone.removeAttribute("data-card-id");
    clone.setAttribute("aria-hidden", "true");
    Object.assign(clone.style, {
      position: "fixed",
      insetBlockStart: "0",
      insetInlineStart: "0",
      inlineSize: `${of.offsetWidth}px`,
      blockSize: `${of.offsetHeight}px`,
      margin: "0",
      transformOrigin: "0 0",
      pointerEvents: "none",
      visibility: "visible",
      zIndex: "27",
    });
    document.body.append(clone);
    return clone;
  }
  const at = (of: HTMLElement, rect: DOMRect) =>
    `translate(${rect.left}px, ${rect.top}px) scale(${rect.width / of.offsetWidth}, ${rect.height / of.offsetHeight})`;
  const liftRect = () => new DOMRect(left, top, width, height);

  function release() {
    if (card) {
      card.style.visibility = "";
      card.style.viewTransitionName = "";
    }
    ghost?.remove();
    ghost = null;
    document.documentElement.classList.remove(MORPH, RETURN);
  }

  onMount(() => {
    card = findCard();
    if (reducedMotion() || !card) {
      // Reduced motion, or a card no longer drawn: the lift crossfades in place.
      shown = true;
      void tick().then(() => {
        panel?.focus({ preventScroll: true });
        const fade = {
          duration: duration(reducedMotion() ? "fast" : "base"),
          easing: easing("out"),
        };
        panel?.animate([{ opacity: 0 }, { opacity: 1 }], fade);
        backdrop?.animate([{ opacity: 0 }, { opacity: 1 }], fade);
      });
      return release;
    }
    const from = card;
    if (morphs()) {
      // The platform morphs the card's snapshot into the lift's; the CSS below times it.
      const root = document.documentElement;
      root.classList.add(MORPH);
      from.style.viewTransitionName = "work-lift";
      const transition = document.startViewTransition(async () => {
        from.style.viewTransitionName = "";
        from.style.visibility = "hidden";
        shown = true;
        await tick();
      });
      void transition.finished.finally(() => {
        root.classList.remove(MORPH);
        panel?.focus({ preventScroll: true });
      });
      return release;
    }
    // Without it, the lift's own box travels from the card while a copy of the card fades.
    const rect = from.getBoundingClientRect();
    from.style.visibility = "hidden";
    shown = true;
    void tick().then(() => {
      if (!panel || !face) return;
      panel.focus({ preventScroll: true });
      const page = duration("page");
      const curve = easing("emphasized");
      panel.animate([{ transform: onto(rect) }, { transform: "none" }], {
        duration: page,
        easing: curve,
      });
      face.animate([{ opacity: 0 }, { opacity: 0, offset: 1 / 3 }, { opacity: 1 }], page);
      backdrop?.animate([{ opacity: 0 }, { opacity: 1 }], { duration: page, easing: curve });
      const clone = (ghost = copy(from));
      clone.animate([{ transform: at(from, rect) }, { transform: at(from, liftRect()) }], {
        duration: page,
        easing: curve,
      });
      void clone
        .animate([{ opacity: 1 }, { opacity: 0 }], { duration: page / 3, fill: "forwards" })
        .finished.then(() => {
          clone.remove();
          if (ghost === clone) ghost = null;
        });
    });
    return release;
  });

  /** Back into the card, wherever it sits now. */
  export function close() {
    if (closing) return;
    closing = true;
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      release();
      onclose();
    };
    const target = card?.isConnected ? card : findCard();
    const element = panel;
    if (!element) return finish();
    if (reducedMotion() || !target) {
      const fade = {
        duration: duration("fast"),
        easing: easing("exit"),
        fill: "forwards" as const,
      };
      element.animate([{ opacity: 1 }, { opacity: 0 }], fade);
      void backdrop?.animate([{ opacity: 1 }, { opacity: 0 }], fade).finished.then(finish, finish);
      setTimeout(finish, duration("fast") + 60);
      return;
    }
    card = target;
    const slow = duration("slow");
    setTimeout(finish, slow + 200);
    if (morphs()) {
      const root = document.documentElement;
      root.classList.add(RETURN);
      try {
        const transition = document.startViewTransition(async () => {
          shown = false;
          target.style.visibility = "";
          target.style.viewTransitionName = "work-lift";
          await tick();
        });
        void transition.finished.finally(finish);
      } catch {
        finish();
      }
      return;
    }
    const curve = easing("exit");
    const rect = (() => {
      target.style.visibility = "";
      const measured = target.getBoundingClientRect();
      target.style.visibility = "hidden";
      return measured;
    })();
    element.animate([{ transform: "none" }, { transform: onto(rect) }], {
      duration: slow,
      easing: curve,
      fill: "forwards",
    });
    face?.animate([{ opacity: 1 }, { opacity: 0, offset: 1 / 3 }, { opacity: 0 }], {
      duration: slow,
      fill: "forwards",
    });
    backdrop?.animate([{ opacity: 1 }, { opacity: 0 }], {
      duration: slow,
      easing: curve,
      fill: "forwards",
    });
    const clone = (ghost = copy(target));
    clone.style.opacity = "0";
    clone.animate([{ transform: at(target, liftRect()) }, { transform: at(target, rect) }], {
      duration: slow,
      easing: curve,
      fill: "forwards",
    });
    void clone
      .animate([{ opacity: 0 }, { opacity: 0, offset: 1 / 3 }, { opacity: 1 }], {
        duration: slow,
        fill: "forwards",
      })
      .finished.then(finish, finish);
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape" && !event.defaultPrevented) {
      event.preventDefault();
      close();
    }
  }}
/>
<div
  class="backdrop"
  class:shown
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
  class:shown
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
  <div class="face" bind:this={face}>
    <button type="button" class="close" aria-label={m.work_env_lift_close()} onclick={close}>
      <Icon icon={Cancel01Icon} size={14} />
    </button>
    <div
      class="content"
      data-lift-scrolled={scrolled || undefined}
      onscroll={(event) => (scrolled = event.currentTarget.scrollTop > 0)}
    >
      {@render children()}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    z-index: 25;
    border-radius: var(--content-radius);
    background: color-mix(in srgb, var(--color-canvas) 55%, transparent);
    visibility: hidden;
  }

  .lift {
    position: fixed;
    z-index: 26;
    display: flex;
    box-sizing: border-box;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow:
      inset 0 0 0 1px var(--color-border),
      var(--shadow-overlay);
    color: var(--color-text);
    transform-origin: top left;
    outline: none;
    overflow: hidden;
    visibility: hidden;
  }

  /* Named only while drawn, so one snapshot never holds the name twice. */
  .backdrop.shown {
    visibility: visible;
    view-transition-name: work-lift-backdrop;
  }

  .lift.shown {
    visibility: visible;
    view-transition-name: work-lift;
  }

  .face {
    position: relative;
    display: flex;
    flex: 1;
    flex-direction: column;
    min-inline-size: 0;
  }

  /* Level with the header's mark, above the header as it sticks. */
  .close {
    position: absolute;
    inset-block-start: 24px;
    inset-inline-end: 24px;
    z-index: 4;
    display: grid;
    place-items: center;
    inline-size: 28px;
    block-size: 28px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-control);
    color: var(--color-muted);
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  .close:hover {
    background: var(--color-control-hover);
    color: var(--color-text);
  }

  .close:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  /* One scroll well for every kind; the header sticks at its top. */
  .content {
    --lift-pad: 24px;

    flex: 1;
    min-block-size: 0;
    padding: var(--lift-pad);
    overflow: auto;
    overscroll-behavior: contain;
  }

  /* The morph: one frame travels from the card's rect to the lift's; the card's
     face is gone by the first third, the lift's arrives over the rest. */
  :global(html.work-lift-morph::view-transition-group(work-lift)),
  :global(html.work-lift-return::view-transition-group(work-lift)) {
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-overlay);
  }

  :global(html.work-lift-morph::view-transition-group(*)) {
    animation-duration: var(--motion-page);
    animation-timing-function: var(--ease-emphasized);
  }

  :global(html.work-lift-return::view-transition-group(*)) {
    animation-duration: var(--motion-slow);
    animation-timing-function: var(--ease-exit);
  }

  :global(html.work-lift-morph::view-transition-old(root)),
  :global(html.work-lift-morph::view-transition-new(root)),
  :global(html.work-lift-return::view-transition-old(root)),
  :global(html.work-lift-return::view-transition-new(root)) {
    animation: none;
  }

  :global(html.work-lift-morph::view-transition-old(work-lift)),
  :global(html.work-lift-morph::view-transition-new(work-lift)),
  :global(html.work-lift-return::view-transition-old(work-lift)),
  :global(html.work-lift-return::view-transition-new(work-lift)) {
    block-size: 100%;
    object-fit: cover;
    object-position: top left;
    mix-blend-mode: normal;
  }

  :global(html.work-lift-morph::view-transition-old(work-lift)) {
    animation: work-lift-out calc(var(--motion-page) / 3) linear both;
  }

  :global(html.work-lift-morph::view-transition-new(work-lift)) {
    animation: work-lift-in calc(var(--motion-page) * 2 / 3) linear calc(var(--motion-page) / 3)
      both;
  }

  :global(html.work-lift-return::view-transition-old(work-lift)) {
    animation: work-lift-out calc(var(--motion-slow) / 3) linear both;
  }

  :global(html.work-lift-return::view-transition-new(work-lift)) {
    animation: work-lift-in calc(var(--motion-slow) * 2 / 3) linear calc(var(--motion-slow) / 3)
      both;
  }

  /* stylelint-disable-next-line keyframes-name-pattern -- Svelte's global prefix. */
  @keyframes -global-work-lift-out {
    to {
      opacity: 0;
    }
  }

  /* stylelint-disable-next-line keyframes-name-pattern -- Svelte's global prefix. */
  @keyframes -global-work-lift-in {
    from {
      opacity: 0;
    }
  }
</style>
