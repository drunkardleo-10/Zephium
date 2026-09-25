<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import { onMount, tick } from "svelte";
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";
  import { BLOCKS, activeBlock, applyBlock, type BlockId } from "../../lib/editor/blocks";
  import { portal } from "../../lib/editor/portal";
  import { boundsOf, place, type Placement } from "../../lib/editor/placement";

  let {
    editor,
    revision,
    anchor,
    prefer,
    onclose,
  }: {
    editor: Editor;
    revision: number;
    /** The control it opens from; it never covers it. */
    anchor: HTMLElement;
    prefer: "above" | "below";
    onclose: (refocus: boolean) => void;
  } = $props();

  let menu = $state<HTMLDivElement>();
  let position = $state<Placement | null>(null);

  let current = $derived.by<BlockId>(() => {
    void revision;
    return activeBlock(editor);
  });

  function reposition() {
    if (!menu) return;
    const rect = anchor.getBoundingClientRect();
    position = place(
      rect,
      { width: menu.offsetWidth, height: menu.scrollHeight },
      boundsOf(anchor),
      prefer,
      { gap: 8 },
    );
  }

  onMount(() => {
    void tick().then(reposition);
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!menu?.contains(target) && !anchor.contains(target)) onclose(false);
    };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("resize", reposition);
    return () => {
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("resize", reposition);
    };
  });

  function items(): HTMLElement[] {
    return [...(menu?.querySelectorAll<HTMLElement>("[data-item]") ?? [])];
  }

  function keydown(event: KeyboardEvent) {
    const all = items();
    const at = all.indexOf(document.activeElement as HTMLElement);
    const step = (delta: number) => {
      event.preventDefault();
      all[(at + delta + all.length) % all.length]?.focus();
    };
    if (event.key === "ArrowDown") step(1);
    else if (event.key === "ArrowUp") step(-1);
    else if (event.key === "Home") step(-at);
    else if (event.key === "End") step(all.length - 1 - at);
    else if (event.key === "Escape" || event.key === "Tab") {
      event.preventDefault();
      event.stopPropagation();
      onclose(true);
    }
  }

  function choose(run: () => void) {
    run();
    onclose(false);
  }
</script>

<!-- Pressing a row keeps the selection in the text, so the choice applies
     to what was selected when the menu opened. -->
<div
  bind:this={menu}
  use:portal
  class="note-float note-block-menu"
  class:above={position?.side === "above"}
  role="menu"
  tabindex="-1"
  aria-label={m.note_formatting()}
  style:left="{position?.left ?? -9999}px"
  style:top="{position?.top ?? -9999}px"
  style:max-height={position?.maxHeight ? `${position.maxHeight}px` : null}
  onmousedown={(event) => event.preventDefault()}
  onkeydown={keydown}
>
  {#each BLOCKS as block (block.id)}
    {#if block.separated}<div class="note-float-separator" role="separator"></div>{/if}
    <button
      type="button"
      role={block.insert ? "menuitem" : "menuitemradio"}
      aria-checked={block.insert ? undefined : current === block.id}
      class="note-float-item"
      data-item
      data-block={block.id}
      tabindex="-1"
      onclick={() => choose(() => applyBlock(editor, block.id))}
    >
      <span class="note-float-icon" aria-hidden="true"><Icon icon={block.icon} size={15} /></span>
      <span class="note-float-label">{block.label()}</span>
      {#if block.hint}<span class="note-float-hint">{block.hint}</span>{/if}
    </button>
  {/each}
</div>
