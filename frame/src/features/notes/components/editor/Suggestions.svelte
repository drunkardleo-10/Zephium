<script lang="ts" module>
  import type { IconSvgElement } from "@hugeicons/svelte";
  export type Suggestion = {
    id: string;
    label: string;
    detail?: string;
    icon: IconSvgElement;
    hint?: string;
    choose: () => void;
  };
</script>

<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import { tick } from "svelte";
  import Icon from "$shared/ui/Icon";
  import { portal } from "../../lib/editor/portal";
  import { boundsOf, place, type Placement } from "../../lib/editor/placement";

  let {
    editor,
    at,
    items,
    label,
    controller,
  }: {
    editor: Editor;
    /** Document position the list hangs from. */
    at: number;
    items: Suggestion[];
    label: string;
    /** Receives the editor's keys while the list is open. */
    controller: { current: ((event: KeyboardEvent) => boolean) | null };
  } = $props();

  let highlighted = $state(0);
  let list = $state<HTMLDivElement>();
  let position = $state<Placement | null>(null);
  const WIDTH = 272;
  const HEIGHT = 296;
  let width = $state(WIDTH);
  const id = `note-suggestions-${Math.random().toString(36).slice(2)}`;

  $effect(() => {
    void items;
    highlighted = 0;
  });

  function reposition() {
    if (!list) return;
    const caret = editor.view.coordsAtPos(at);
    const bounds = boundsOf(editor.view.dom);
    width = Math.min(WIDTH, bounds.right - bounds.left);
    position = place(
      { ...caret, left: caret.left - 10 },
      { width, height: Math.min(list.scrollHeight, HEIGHT) },
      bounds,
      "below",
    );
  }

  $effect(() => {
    void at;
    void items.length;
    void tick().then(reposition);
    const again = () => reposition();
    window.addEventListener("scroll", again, { capture: true, passive: true });
    return () => window.removeEventListener("scroll", again, { capture: true });
  });

  $effect(() => {
    controller.current = (event) => {
      if (!items.length) return false;
      if (event.key === "ArrowDown" || (event.key === "n" && event.ctrlKey)) {
        highlighted = (highlighted + 1) % items.length;
      } else if (event.key === "ArrowUp" || (event.key === "p" && event.ctrlKey)) {
        highlighted = (highlighted - 1 + items.length) % items.length;
      } else if (event.key === "Enter" || event.key === "Tab") {
        items[highlighted]?.choose();
      } else return false;
      void tick().then(() =>
        list?.querySelector(`[data-index="${highlighted}"]`)?.scrollIntoView({ block: "nearest" }),
      );
      return true;
    };
    return () => {
      controller.current = null;
    };
  });
</script>

{#if items.length}
  <div
    bind:this={list}
    use:portal
    {id}
    class="note-float note-suggestions"
    class:above={position?.side === "above"}
    role="listbox"
    aria-label={label}
    tabindex="-1"
    style:left="{position?.left ?? -9999}px"
    style:top="{position?.top ?? -9999}px"
    style:width="{width}px"
    style:max-height="{position?.maxHeight ?? HEIGHT}px"
    onmousedown={(event) => event.preventDefault()}
  >
    {#each items as item, index (item.id)}
      <div
        class="note-float-item"
        role="option"
        tabindex="-1"
        aria-selected={index === highlighted}
        data-index={index}
        data-highlighted={index === highlighted || undefined}
        onmousemove={() => (highlighted = index)}
        onclick={item.choose}
        onkeydown={() => {}}
      >
        <span class="note-float-icon" aria-hidden="true"><Icon icon={item.icon} size={15} /></span>
        <span class="note-float-label">
          <span>{item.label}</span>
          {#if item.detail}<span class="note-float-detail">{item.detail}</span>{/if}
        </span>
        {#if item.hint}<span class="note-float-hint">{item.hint}</span>{/if}
      </div>
    {/each}
  </div>
{/if}
