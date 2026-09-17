<script lang="ts">
  import { getContext } from "svelte";
  import { canvasAuthor } from "../../lib/canvas-context";
  import type { CanvasItem } from "../../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let { item, selected, onaction }: { item: CanvasItem; selected: boolean; onaction: () => void } =
    $props();
  const author = getContext<{ readonly initial: string } | undefined>(canvasAuthor);
</script>

<article class="request" class:selected>
  <div class="work-drag-handle grip"></div>
  <header>
    <span class="you" title={m.work_env_you()}>{author?.initial || "•"}</span>
    <span class="who">{m.work_env_you()}</span>
  </header>
  <p class="sentence">{item.title}</p>
  {#if item.actionLabel}<footer>
      <button
        type="button"
        class="link nodrag nopan"
        onclick={(event) => {
          event.stopPropagation();
          onaction();
        }}>{item.actionLabel}</button
      >
    </footer>{/if}
</article>

<style>
  /* The person's words, on the one surface no machine card uses. */
  .request {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 6px;
    box-sizing: border-box;
    block-size: 100%;
    padding: 14px 16px 12px;
    border-radius: var(--radius-lg);
    background:
      linear-gradient(
        160deg,
        color-mix(in srgb, var(--color-accent) 10%, transparent),
        transparent 62%
      ),
      var(--color-raised);
    box-shadow:
      inset 0 0 0 1px var(--color-border),
      var(--shadow-float);
    color: var(--color-text);
    overflow: hidden;
  }

  .request.selected {
    box-shadow:
      inset 0 0 0 1px var(--color-border-strong),
      0 0 0 2px var(--color-accent-soft),
      0 0 0 3px var(--color-accent);
  }

  .grip {
    position: absolute;
    inset: 0;
    cursor: grab;
  }

  .grip:active {
    cursor: grabbing;
  }

  header {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
    pointer-events: none;
  }

  .you {
    display: grid;
    place-items: center;
    inline-size: 20px;
    block-size: 20px;
    border-radius: 50%;
    background: var(--color-fill-active);
    color: var(--color-label-secondary);
    font-size: var(--text-caption);
    font-weight: 600;
  }

  .who {
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .sentence {
    position: relative;
    flex: 1;
    min-block-size: 0;
    margin: 0;
    font-size: 15px;
    font-weight: 450;
    line-height: 20px;
    letter-spacing: -0.008em;
    pointer-events: none;
    overflow: hidden;
  }

  footer {
    position: relative;
    display: flex;
    justify-content: flex-end;
    flex: none;
  }

  .link {
    border: 0;
    padding: 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-caption);
    cursor: default;
  }

  .link:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }
</style>
