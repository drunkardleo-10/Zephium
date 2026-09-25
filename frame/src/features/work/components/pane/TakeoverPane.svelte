<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { WorkHumanAccountV1 } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import SegmentedControl from "$shared/ui/SegmentedControl";
  import { paneGeometry, remember, type PaneRect } from "../../lib/pane-geometry";
  import { countdownLabel, reasonSentence, type HumanPage } from "../../lib/work-human";
  import * as m from "$shared/i18n/messages";
  let {
    host,
    url,
    bounds,
    page,
    error = null,
    onregion,
    oncontinue,
    onclose,
  }: {
    host: string;
    url: string;
    /** The canvas card the pane floats over, as the browser pane uses it. */
    bounds: DOMRect;
    page: HumanPage;
    error?: string | null;
    /** The well in window logical points, or null while it is not stable. */
    onregion: (rect: PaneRect | null) => void;
    oncontinue: (account: WorkHumanAccountV1) => void;
    onclose: () => void;
  } = $props();
  const MIN = { width: 520, height: 420 };
  let rect = $state.raw<PaneRect>(untrack(() => paneGeometry(bounds)));
  let heading = $state<HTMLElement>();
  let well = $state<HTMLElement>();
  let closing = $state(false);
  let account = $state<WorkHumanAccountV1>("anonymous");
  const handingBack = $derived(page.phase === "continuing");
  const presented = $derived(page.phase === "presented");
  const countdown = $derived(countdownLabel(page.remaining));
  /** Only a sign-in has an account to keep; every other page hands back anonymously. */
  const signIn = $derived(page.reason === "sign_in");
  const accounts = $derived([
    { value: "anonymous", label: m.work_human_account_anonymous() },
    { value: "signed_in_public_only", label: m.work_human_account_signed_in() },
  ]);
  function clamp(next: PaneRect): PaneRect {
    const width = Math.max(MIN.width, Math.min(next.width, bounds.width));
    const height = Math.max(MIN.height, Math.min(next.height, bounds.height));
    return {
      x: Math.min(Math.max(next.x, bounds.left), Math.max(bounds.left, bounds.right - width)),
      y: Math.min(Math.max(next.y, bounds.top), Math.max(bounds.top, bounds.bottom - height)),
      width,
      height,
    };
  }
  $effect(() => {
    void bounds;
    const next = clamp(untrack(() => rect));
    if (
      next.x !== rect.x ||
      next.y !== rect.y ||
      next.width !== rect.width ||
      next.height !== rect.height
    )
      rect = next;
  });
  let frame = 0;
  function measure() {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      const element = well;
      if (!element || closing) return;
      if (drag || handingBack) {
        onregion(null);
        return;
      }
      const box = element.getBoundingClientRect();
      if (box.width < 1 || box.height < 1) return;
      onregion({ x: box.left, y: box.top, width: box.width, height: box.height });
    });
  }
  $effect(() => {
    const element = well;
    if (!element) return;
    const observer = new ResizeObserver(() => measure());
    observer.observe(element);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  });
  $effect(() => {
    remember(rect);
    // The region follows the pane, the drag, and the hand-back; nothing else.
    void drag;
    void handingBack;
    measure();
  });
  onMount(() => {
    heading?.focus({ preventScroll: true });
  });
  function close() {
    if (closing) return;
    closing = true;
    onclose();
  }
  type Drag = {
    kind: "move" | "resize";
    edge: string;
    startX: number;
    startY: number;
    from: PaneRect;
  };
  let drag = $state.raw<Drag | null>(null);
  function capture(event: PointerEvent) {
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {
      /* synthetic pointers have no capture */
    }
  }
  function beginMove(event: PointerEvent) {
    if (event.button !== 0) return;
    if ((event.target as HTMLElement).closest("button, input, a")) return;
    drag = { kind: "move", edge: "", startX: event.clientX, startY: event.clientY, from: rect };
    capture(event);
  }
  function beginResize(event: PointerEvent, edge: string) {
    if (event.button !== 0) return;
    drag = { kind: "resize", edge, startX: event.clientX, startY: event.clientY, from: rect };
    capture(event);
    event.preventDefault();
  }
  function move(event: PointerEvent) {
    const current = drag;
    if (!current) return;
    const dx = event.clientX - current.startX;
    const dy = event.clientY - current.startY;
    const from = current.from;
    if (current.kind === "move") {
      rect = clamp({ ...from, x: from.x + dx, y: from.y + dy });
      return;
    }
    let { x, y, width, height } = from;
    if (current.edge.includes("e")) width = from.width + dx;
    if (current.edge.includes("s")) height = from.height + dy;
    if (current.edge.includes("w")) {
      width = from.width - dx;
      x = from.x + Math.min(dx, from.width - MIN.width);
    }
    if (current.edge.includes("n")) {
      height = from.height - dy;
      y = from.y + Math.min(dy, from.height - MIN.height);
    }
    rect = clamp({ x, y, width, height });
  }
  const end = () => (drag = null);
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape" && !event.defaultPrevented) {
      event.preventDefault();
      close();
    }
  }}
/>
<section
  class="takeover"
  aria-label={m.work_human_pane()}
  style:left={`${rect.x}px`}
  style:top={`${rect.y}px`}
  style:inline-size={`${rect.width}px`}
  style:block-size={`${rect.height}px`}
>
  <header
    bind:this={heading}
    tabindex="-1"
    class="head"
    role="toolbar"
    aria-label={m.work_pane_move()}
    onpointerdown={beginMove}
    onpointermove={move}
    onpointerup={end}
    onpointercancel={end}
  >
    <span class="titles">
      <span class="who" title={url}>{host}</span>
      <span class="why">{reasonSentence(page.reason, host)}</span>
    </span>
    {#if countdown}<span class="left">{countdown}</span>{/if}
  </header>
  <div class="prompt">
    {#if handingBack}
      <p class="handing" role="status">{m.work_human_handing_back()}</p>
    {:else}
      {#if signIn}
        <SegmentedControl
          label={m.work_human_account()}
          options={accounts}
          bind:value={() => account, (next) => (account = next as WorkHumanAccountV1)}
        />
        <p class="help">
          {account === "anonymous"
            ? m.work_human_account_anonymous_help()
            : m.work_human_account_signed_in_help()}
        </p>
      {/if}
      <div class="actions">
        <Button
          variant="primary"
          size="compact"
          disabled={!page.canContinue}
          onclick={() => oncontinue(signIn ? account : "anonymous")}
          >{m.work_human_continue()}</Button
        >
        <Button size="compact" onclick={close}>{m.work_human_release()}</Button>
        <span class="skip">{m.work_human_release_help()}</span>
      </div>
    {/if}
    {#if error}<p class="failed" role="alert">{error}</p>{/if}
  </div>
  <div
    bind:this={well}
    class="well"
    class:presented
    role="group"
    aria-label={m.work_human_region()}
  >
    {#if !presented && !handingBack}<p class="placeholder">{m.work_human_opening()}</p>{/if}
  </div>
  {#each ["n", "e", "s", "w", "ne", "se", "sw", "nw"] as edge (edge)}
    <span
      class={`grip grip-${edge}`}
      role="presentation"
      onpointerdown={(event) => beginResize(event, edge)}
      onpointermove={move}
      onpointerup={end}
      onpointercancel={end}
    ></span>
  {/each}
</section>

<style>
  /* The well never moves or fades: the agent's own view sits in it. Only the
     chrome around it arrives. */
  .takeover {
    position: fixed;
    z-index: 28;
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    box-shadow:
      0 0 0 1px var(--color-border-strong),
      var(--shadow-popover);
    color: var(--color-text);
    overflow: hidden;
    contain: layout;
  }

  .head {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    box-sizing: border-box;
    min-block-size: 44px;
    padding: 8px 14px;
    background: color-mix(in srgb, var(--color-raised) 70%, var(--color-surface));
    cursor: grab;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    outline: none;
    animation: takeover-chrome var(--motion-base) var(--ease-smooth) both;
  }

  .head:active {
    cursor: grabbing;
  }

  .titles {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-inline-size: 0;
  }

  .who {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 600;
    line-height: 13px;
  }

  .why {
    font-size: var(--text-body);
    font-weight: 500;
    line-height: 17px;
    text-wrap: pretty;
  }

  .prompt {
    display: flex;
    flex: 0 0 auto;
    flex-direction: column;
    gap: 10px;
    padding: 12px 14px 14px;
    animation: takeover-chrome var(--motion-base) var(--ease-smooth) 40ms both;
  }

  .left {
    flex: none;
    color: var(--color-muted);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
  }

  .help {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
  }

  .handing,
  .failed {
    margin: 0;
    font-size: var(--text-label);
  }

  .handing {
    color: var(--color-muted);
  }

  .failed {
    color: var(--color-danger);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .skip {
    min-inline-size: 0;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .well {
    position: relative;
    flex: 1 1 auto;
    min-block-size: 64px;
    margin: 0 1px 1px;
    border-radius: 0 0 calc(var(--radius-lg) - 1px) calc(var(--radius-lg) - 1px);
    background: var(--color-page);
    box-shadow: inset 0 0 0 1px var(--color-border);
  }

  .well.presented {
    background: transparent;
  }

  .placeholder {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .grip {
    position: absolute;
    z-index: 2;
  }

  .grip-n,
  .grip-s {
    inset-inline: 10px;
    block-size: 6px;
    cursor: ns-resize;
  }

  .grip-e,
  .grip-w {
    inset-block: 10px;
    inline-size: 6px;
    cursor: ew-resize;
  }

  .grip-n {
    inset-block-start: 0;
  }

  .grip-s {
    inset-block-end: 0;
  }

  .grip-e {
    inset-inline-end: 0;
  }

  .grip-w {
    inset-inline-start: 0;
  }

  .grip-ne,
  .grip-se,
  .grip-sw,
  .grip-nw {
    inline-size: 12px;
    block-size: 12px;
  }

  .grip-ne {
    inset-block-start: 0;
    inset-inline-end: 0;
    cursor: nesw-resize;
  }

  .grip-sw {
    inset-block-end: 0;
    inset-inline-start: 0;
    cursor: nesw-resize;
  }

  .grip-se {
    inset-block-end: 0;
    inset-inline-end: 0;
    cursor: nwse-resize;
  }

  .grip-nw {
    inset-block-start: 0;
    inset-inline-start: 0;
    cursor: nwse-resize;
  }

  @keyframes takeover-chrome {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .head,
    .prompt {
      animation: none;
    }
  }
</style>
