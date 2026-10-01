<script lang="ts">
  import { onMount, untrack } from "svelte";
  import type { TabView } from "$shared/ipc/bindings";
  import type { WorkPaneHole } from "$domain/layout";
  import FavIcon from "$shared/ui/FavIcon";
  import { favicons } from "$domain/favicons";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";
  import Button from "$shared/ui/Button";
  import {
    ArrowLeft02Icon,
    ArrowReloadHorizontalIcon,
    ArrowRight02Icon,
    ArrowUpRight01Icon,
    Cancel01Icon,
    GlobalIcon,
    LockIcon,
    PlusSignIcon,
    Search01Icon,
  } from "../../lib/icons";
  import { paneGeometry, remember, type PaneRect } from "../../lib/pane-geometry";
  import AccountBadge from "../cards/AccountBadge.svelte";
  import * as m from "$shared/i18n/messages";
  let {
    tab,
    account,
    applied,
    bounds,
    origin,
    phase,
    added = false,
    onmeasure,
    onnavigate,
    onback,
    onforward,
    onreload,
    onopenbrowse,
    onadd,
    onclose,
  }: {
    tab: TabView | undefined;
    /** The card was read with the person's session on this host; the badge follows it here. */
    account?: string;
    applied: WorkPaneHole | null;
    bounds: DOMRect;
    origin: DOMRect | null;
    phase: "opening" | "shown" | "failed";
    added?: boolean;
    onmeasure: (rect: PaneRect) => void;
    onnavigate: (input: string) => void;
    onback: () => void;
    onforward: () => void;
    onreload: () => void;
    onopenbrowse: () => void;
    onadd: () => void;
    onclose: () => void;
  } = $props();
  const HEADER = 44;
  const MIN = { width: 482, height: 322 + HEADER };
  let rect = $state.raw<PaneRect>(untrack(() => paneGeometry(bounds)));
  let root = $state<HTMLElement>();
  let body = $state<HTMLElement>();
  let heading = $state<HTMLElement>();
  let address = $state<HTMLInputElement>();
  let editing = $state(false);
  let draft = $state("");
  let closing = $state(false);
  let settled = $state(false);
  const url = $derived(tab?.url ?? "");
  const host = $derived.by(() => {
    try {
      return url ? new URL(url).host : "";
    } catch {
      return "";
    }
  });
  const secure = $derived(url.startsWith("https:") || url.startsWith("wss:"));
  const value = $derived(editing ? draft : host);
  const presented = $derived(phase === "shown" && !!applied?.presented);
  const reduce = () =>
    window.matchMedia("(prefers-reduced-motion: reduce)").matches ||
    document.documentElement.dataset.reduceMotion === "true";
  let frame = 0;
  function measure() {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      const element = body;
      if (!element || closing || !settled) return;
      const box = element.getBoundingClientRect();
      if (box.width < 1 || box.height < 1) return;
      onmeasure({ x: box.left, y: box.top, width: box.width, height: box.height });
    });
  }
  function clamp(next: PaneRect): PaneRect {
    const width = Math.max(MIN.width, Math.min(next.width, bounds.width));
    const height = Math.max(MIN.height, Math.min(next.height, bounds.height));
    return {
      x: Math.min(Math.max(next.x, bounds.left), bounds.right - width),
      y: Math.min(Math.max(next.y, bounds.top), bounds.bottom - height),
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
  const expectedHole = (box: PaneRect): PaneRect => ({
    x: box.x + 1,
    y: box.y + HEADER,
    width: box.width - 2,
    height: box.height - HEADER - 1,
  });
  const near = (a: PaneRect, b: PaneRect) =>
    Math.abs(a.x - b.x) <= 0.5 &&
    Math.abs(a.y - b.y) <= 0.5 &&
    Math.abs(a.width - b.width) <= 0.5 &&
    Math.abs(a.height - b.height) <= 0.5;
  // Recent intents, so an echoed layout is not mistaken for a native clamp.
  const requested: PaneRect[] = [];
  $effect(() => {
    requested.push(expectedHole(rect));
    if (requested.length > 12) requested.shift();
  });
  $effect(() => {
    const hole = applied;
    if (!hole || phase !== "shown" || closing || drag) return;
    if (requested.some((intent) => near(intent, hole))) return;
    rect = {
      x: hole.x - 1,
      y: hole.y - HEADER,
      width: hole.width + 2,
      height: hole.height + HEADER + 1,
    };
  });
  $effect(() => {
    const element = body;
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
    measure();
  });
  onMount(() => {
    heading?.focus({ preventScroll: true });
    const element = root;
    if (!element || reduce() || !origin) {
      settled = true;
      measure();
      return;
    }
    const sx = origin.width / rect.width;
    const sy = origin.height / rect.height;
    try {
      element
        .animate(
          [
            {
              transform: `translate(${origin.left - rect.x}px, ${origin.top - rect.y}px) scale(${sx}, ${sy})`,
              opacity: 0.4,
            },
            { transform: "none", opacity: 1 },
          ],
          { duration: 260, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
        )
        .finished.finally(() => {
          settled = true;
          measure();
        });
    } catch {
      settled = true;
      measure();
    }
  });
  export function close() {
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
  function beginMove(event: PointerEvent) {
    if (event.button !== 0) return;
    if ((event.target as HTMLElement).closest("button, input, a")) return;
    drag = { kind: "move", edge: "", startX: event.clientX, startY: event.clientY, from: rect };
    capture(event);
  }
  function capture(event: PointerEvent) {
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {
      /* synthetic pointers have no capture */
    }
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
  function end() {
    drag = null;
  }
  function submit(event: SubmitEvent) {
    event.preventDefault();
    const input = value.trim();
    editing = false;
    address?.blur();
    if (input) onnavigate(input);
  }
  function beginEditing() {
    draft = url || value;
    editing = true;
    address?.select();
  }
  function onAddressKey(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    editing = false;
    address?.blur();
    heading?.focus({ preventScroll: true });
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
{#if tab && !closing}
  <div
    class="sentinel"
    aria-hidden="true"
    data-zephium-tab-id={tab.id}
    data-zephium-tab-url={tab.url ?? ""}
    data-zephium-projection-revision={tab.projection_revision}
  >
    <span data-zephium-tab-label>{tab.title}</span>
  </div>
{/if}
<section
  bind:this={root}
  class="pane"
  class:presented
  aria-label={m.work_pane()}
  style:left={`${rect.x}px`}
  style:top={`${rect.y}px`}
  style:inline-size={`${rect.width}px`}
  style:block-size={`${rect.height}px`}
  style:--pane-header={`${HEADER}px`}
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
    <span class="identity" title={tab?.title ?? ""}>
      <FavIcon
        image={favicons.image(tab?.icon ?? null)}
        tone={favicons.tone(tab?.icon ?? null)}
        loading={tab?.loading ?? false}
        size={16}
        lit
      />{#if account && account === host}<AccountBadge host={account} />{/if}
    </span>
    <span class="nav">
      <IconButton
        icon={ArrowLeft02Icon}
        label={m.work_pane_back()}
        disabled={!tab?.can_go_back}
        onclick={onback}
      />
      <IconButton
        icon={ArrowRight02Icon}
        label={m.work_pane_forward()}
        disabled={!tab?.can_go_forward}
        onclick={onforward}
      />
      <IconButton
        icon={ArrowReloadHorizontalIcon}
        label={m.work_pane_reload()}
        disabled={!tab}
        onclick={onreload}
      />
    </span>
    <form class="address" class:editing onsubmit={submit} role="search">
      <span class="lead" aria-hidden="true">
        <Icon icon={editing || !url ? Search01Icon : secure ? LockIcon : GlobalIcon} size={13} />
      </span>
      <input
        bind:this={address}
        data-zephium-address
        type="text"
        aria-label={m.work_pane_address()}
        autocomplete="off"
        autocapitalize="off"
        enterkeyhint="go"
        spellcheck="false"
        placeholder={m.ui_search_or_enter_an_address()}
        {value}
        oninput={(event) => (draft = event.currentTarget.value)}
        onfocus={beginEditing}
        onblur={() => (editing = false)}
        onkeydown={onAddressKey}
      />
    </form>
    <span class="actions">
      <Button size="compact" disabled={!tab || added} onclick={onadd}
        ><Icon icon={PlusSignIcon} size={13} />{added
          ? m.work_pane_added()
          : m.work_pane_add()}</Button
      >
      <Button size="compact" disabled={!tab} onclick={onopenbrowse}
        ><Icon icon={ArrowUpRight01Icon} size={13} />{m.work_env_open_browse()}</Button
      >
      <IconButton icon={Cancel01Icon} label={m.work_pane_close()} onclick={close} />
    </span>
  </header>
  <div bind:this={body} class="body" aria-live="polite">
    {#if !presented}
      <p class="placeholder">
        {#if phase === "failed"}{m.work_pane_failed()}{:else if phase === "opening" || !applied}{m.work_pane_opening()}{:else}{m.work_pane_unavailable()}{/if}
      </p>
    {/if}
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
  .sentinel {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  .pane {
    position: fixed;
    z-index: 28;
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow:
      0 0 0 1px var(--color-border-strong),
      var(--shadow-popover);
    color: var(--color-text);
    overflow: hidden;
    contain: layout;
  }

  .head {
    position: relative;
    display: flex;
    flex: 0 0 var(--pane-header);
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    padding: 0 8px 0 10px;
    background: color-mix(in srgb, var(--color-raised) 70%, var(--color-surface));
    cursor: grab;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    outline: none;
  }

  .head:active {
    cursor: grabbing;
  }

  .identity {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-inline-size: 20px;
    justify-content: center;
    color: var(--color-muted);
  }

  .nav,
  .actions {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    flex: 0 0 auto;
  }

  .actions {
    gap: 6px;
  }

  .address {
    display: flex;
    flex: 1 1 auto;
    align-items: center;
    gap: 6px;
    min-inline-size: 120px;
    block-size: 28px;
    margin: 0;
    padding: 0 10px;
    border-radius: var(--radius-control-compact);
    background: var(--color-field);
    box-shadow: var(--shadow-field);
    cursor: text;
    transition:
      background-color var(--motion-fast) var(--ease-smooth),
      box-shadow var(--motion-fast) var(--ease-smooth);
  }

  .address:hover {
    background: var(--color-field-hover);
  }

  .address.editing,
  .address:focus-within {
    box-shadow: var(--shadow-field-focus);
  }

  .lead {
    display: inline-flex;
    color: var(--color-faint);
  }

  .address input {
    flex: 1 1 auto;
    min-inline-size: 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    outline: none;
  }

  .address input::placeholder {
    color: var(--color-faint);
  }

  .body {
    position: relative;
    flex: 1 1 auto;
    min-block-size: 0;
    margin: 0 1px 1px;
    border-radius: 0 0 calc(var(--radius-card) - 1px) calc(var(--radius-card) - 1px);
    background: var(--color-page);
    transition: background-color var(--motion-fast) var(--ease-smooth);
  }

  .pane.presented .body {
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
</style>
