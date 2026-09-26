<script lang="ts">
  import { onMount } from "svelte";
  import { useSvelteFlow, useViewport, Panel } from "@xyflow/svelte";
  import Icon from "$shared/ui/Icon";
  import { duration, reducedMotion } from "$shared/lib/motion";
  import { MinusSignIcon, PlusSignIcon } from "../lib/icons";
  import type { PointerTool } from "../lib/selection";
  import * as m from "$shared/i18n/messages";
  let {
    bottomInset = 0,
  }: {
    bottomInset?: number;
    /** No longer drawn: panning is the default, Shift draws a marquee, Space pans. */
    tool?: PointerTool;
    ontool?: (tool: PointerTool) => void;
  } = $props();
  const flow = useSvelteFlow();
  const viewport = useViewport();
  const percent = $derived(Math.round(viewport.current.zoom * 100));
  /** A mark inside the flow: the capsule itself may be drawn in the chrome's row. */
  let anchor = $state<HTMLElement>();
  /** The chrome row's slot for the capsule, when this canvas sits under one. */
  let slot = $state<HTMLElement | null>(null);
  onMount(() => {
    const find = () => {
      for (let at = anchor?.parentElement; at; at = at.parentElement) {
        const found = at.querySelector<HTMLElement>("[data-work-zoom-slot]");
        if (found) return found;
      }
      return null;
    };
    slot = find();
    if (slot) return;
    // The chrome can arrive a frame after the canvas.
    const frame = requestAnimationFrame(() => (slot = find()));
    return () => cancelAnimationFrame(frame);
  });
  /** Draws the capsule in the chrome's slot. */
  function into(node: HTMLElement, target: HTMLElement) {
    // One element moves, so Svelte's removal of the block still finds it.
    target.append(node);
    return { destroy: () => node.remove() };
  }
  /** The canvas the pointer last pressed in, so a key reaches it while nothing else has focus. */
  let engaged = false;
  const travel = (name: "fast" | "slow") => (reducedMotion() ? 0 : duration(name));
  function fit() {
    void flow.fitView({
      duration: travel("slow"),
      padding:
        bottomInset > 0
          ? { top: "110px", bottom: `${bottomInset + 32}px`, left: "40px", right: "40px" }
          : 0.2,
    });
  }
  /** 0 fits the view and 1 shows it at actual size, while the canvas holds the keyboard. */
  function keys(event: KeyboardEvent) {
    if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return;
    if (event.key !== "0" && event.key !== "1") return;
    const canvas = anchor?.closest(".svelte-flow");
    const active = document.activeElement;
    const inside = !!active && !!canvas?.contains(active);
    if (!inside && !(engaged && (!active || active === document.body))) return;
    if ((event.target as Element | null)?.closest?.("input, textarea, select, [contenteditable]"))
      return;
    event.preventDefault();
    if (event.key === "0") fit();
    else void flow.setZoom(1, { duration: travel("slow") });
  }
</script>

<svelte:window
  onpointerdowncapture={(event) =>
    (engaged = !!anchor?.closest(".svelte-flow")?.contains(event.target as Node))}
  onkeydown={keys}
/>

<span class="anchor" hidden bind:this={anchor}></span>
{#snippet capsule(docked: boolean)}
  <div class="capsule" class:docked role="group" aria-label={m.work_canvas_controls()}>
    <button
      type="button"
      aria-label={m.work_zoom_out()}
      title={m.work_zoom_out()}
      onclick={() => void flow.zoomOut({ duration: travel("fast") })}
      ><Icon icon={MinusSignIcon} size={12} /></button
    >
    <button type="button" class="level" aria-label={m.work_fit()} title={m.work_fit()} onclick={fit}
      >{percent}%</button
    >
    <button
      type="button"
      aria-label={m.work_zoom_in()}
      title={m.work_zoom_in()}
      onclick={() => void flow.zoomIn({ duration: travel("fast") })}
      ><Icon icon={PlusSignIcon} size={12} /></button
    >
  </div>
{/snippet}
{#if slot}<div class="docked-host" use:into={slot}>{@render capsule(true)}</div>
{:else}<Panel position="bottom-left">{@render capsule(false)}</Panel>{/if}

<style>
  .docked-host {
    display: flex;
  }

  .capsule {
    display: inline-flex;
    align-items: center;
    box-sizing: border-box;
    block-size: 32px;
    padding: 2px;
    border-radius: var(--radius-capsule);
    background: var(--color-float);
    box-shadow: var(--shadow-popover);
  }

  button {
    display: grid;
    place-items: center;
    min-inline-size: 28px;
    block-size: 28px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-capsule);
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    cursor: default;
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }

  button:hover {
    background: var(--color-control-hover);
    color: var(--color-text);
  }

  button:active {
    background: var(--color-control-pressed);
  }

  button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .level {
    min-inline-size: 48px;
    padding-inline: 6px;
    color: var(--color-text);
    font-size: var(--text-label);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  /* In the chrome row: the tasks pill's shape and fill, no float. */
  .capsule.docked {
    block-size: 26px;
    padding: 0 2px;
    background: var(--color-fill);
    box-shadow: none;
  }

  .docked button {
    min-inline-size: 22px;
    block-size: 22px;
  }

  .docked .level {
    min-inline-size: 44px;
    color: var(--color-label-secondary);
  }

  .docked .level:hover {
    color: var(--color-text);
  }
</style>
