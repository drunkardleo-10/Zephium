<script lang="ts">
  import {
    applyDragWidth,
    COMPACT_WIDTH,
    MAX_EXPANDED_WIDTH,
    MIN_EXPANDED_WIDTH,
  } from "./sidebar-mode.svelte";

  const minimum = COMPACT_WIDTH;
  const maximum = MAX_EXPANDED_WIDTH;

  let { width }: { width: number } = $props();

  let resizing = false;
  let startX = 0;
  let startWidth = 0;
  let frame = 0;
  let pendingWidth = 0;

  // The sidebar has two designed shapes and nothing in between, so a drag
  // resolves to one of them rather than to a raw pixel width.
  function update(next: number, commit: boolean) {
    pendingWidth = next;
    if (commit) {
      if (frame !== 0) {
        cancelAnimationFrame(frame);
        frame = 0;
      }
      applyDragWidth(next);
      return;
    }
    if (frame !== 0) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      applyDragWidth(pendingWidth);
    });
  }

  function handlePointerDown(event: PointerEvent) {
    const target = event.currentTarget;
    if (!(target instanceof HTMLElement)) return;
    resizing = true;
    startX = event.clientX;
    startWidth = width;
    target.setPointerCapture(event.pointerId);
  }

  function handlePointerMove(event: PointerEvent) {
    if (!resizing) return;
    update(startWidth + event.clientX - startX, false);
  }

  function endPointerResize(event: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    const target = event.currentTarget;
    if (target instanceof HTMLElement && target.hasPointerCapture(event.pointerId)) {
      target.releasePointerCapture(event.pointerId);
    }
    update(startWidth + event.clientX - startX, true);
  }

  function cancelPointerResize(event: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    const target = event.currentTarget;
    if (target instanceof HTMLElement && target.hasPointerCapture(event.pointerId)) {
      target.releasePointerCapture(event.pointerId);
    }
    update(width, true);
  }

  function handleKeydown(event: KeyboardEvent) {
    const delta = event.shiftKey ? 24 : 8;
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      update(width - delta, true);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      update(width + delta, true);
    } else if (event.key === "Home") {
      event.preventDefault();
      update(MIN_EXPANDED_WIDTH, true);
    } else if (event.key === "End") {
      event.preventDefault();
      update(maximum, true);
    }
  }
</script>

<!-- An ARIA separator is interactive only when focusable and value-bearing.
     Svelte's static rule does not model that WAI-ARIA pattern. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  role="separator"
  aria-label="Resize sidebar"
  aria-orientation="vertical"
  aria-valuemin={minimum}
  aria-valuemax={maximum}
  aria-valuenow={width}
  tabindex="0"
  class="group absolute top-0 right-0 z-20 h-full w-1.5 translate-x-1/2 cursor-col-resize touch-none outline-none"
  onpointerdown={handlePointerDown}
  onpointermove={handlePointerMove}
  onpointerup={endPointerResize}
  onpointercancel={cancelPointerResize}
  onkeydown={handleKeydown}
>
  <span
    aria-hidden="true"
    class="absolute top-0 bottom-0 left-1/2 w-px -translate-x-1/2 bg-transparent transition-colors group-hover:bg-border-strong group-focus-visible:bg-accent"
  ></span>
</div>
