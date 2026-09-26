<script lang="ts">
  import { getContext } from "svelte";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "$shared/ui/Icon";
  import FavIcon from "$shared/ui/FavIcon";
  import {
    AccountSetting01Icon,
    Alert02Icon,
    BrowserIcon,
    Clock01Icon,
    ComputerTerminal01Icon,
    File01Icon,
    HelpCircleIcon,
    Message01Icon,
    Search01Icon,
    SparklesIcon,
  } from "../../lib/icons";
  import { canvasBoard, type BoardActions } from "../../lib/canvas-context";
  import type { CanvasItem } from "../../lib/canvas-model";
  import type { TrailIcon } from "../../lib/board/trail";
  let { item, selected = false }: { item: CanvasItem; selected?: boolean } = $props();
  const actions = getContext<BoardActions | undefined>(canvasBoard);
  const ICONS: Record<Exclude<TrailIcon, "live">, IconSvgElement> = {
    search: Search01Icon,
    page: BrowserIcon,
    knowledge: SparklesIcon,
    ask: HelpCircleIcon,
    steer: Message01Icon,
    file: File01Icon,
    command: ComputerTerminal01Icon,
    account: AccountSetting01Icon,
    time: Clock01Icon,
    stopped: Alert02Icon,
  };
</script>

<!-- What the request's runs did, as closed facts; while one runs, the line it is on moves. -->
<ol class="trail work-drag-handle" class:selected class:active={item.active} aria-label={item.kind}>
  {#each item.trail ?? [] as line (line.key)}
    <li class:live={line.live}>
      <span class="glyph" aria-hidden="true"
        >{#if line.icon === "live"}<FavIcon image={null} loading size={14} />{:else}<Icon
            icon={ICONS[line.icon]}
            size={14}
          />{/if}</span
      >
      <span class="words">
        {#if line.command}<button
            type="button"
            class="text code nodrag nopan"
            onclick={() => actions?.command(line.command!)}>{line.text}</button
          >{:else}<span class="text" class:code={line.code}>{line.text}</span>{/if}
        {#if line.detail}<span class="detail">{line.detail}</span>{/if}
      </span>
    </li>
  {/each}
</ol>

<style>
  .trail {
    display: flex;
    flex-direction: column;
    gap: 6px;
    box-sizing: border-box;
    block-size: 100%;
    margin: 0;
    padding: 12px;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: inset 0 0 0 1px var(--color-border);
    list-style: none;
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  .trail.selected {
    box-shadow:
      0 0 0 1px var(--color-lit),
      var(--shadow-raised);
  }

  li {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    min-inline-size: 0;
  }

  .glyph {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 20px;
    block-size: 20px;
    color: var(--color-faint);
  }

  .words {
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
  }

  .text {
    overflow: hidden;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-label-secondary);
    font: inherit;
    font-size: var(--text-label);
    line-height: 20px;
    text-align: start;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .live .text {
    color: var(--color-text);
    font-weight: 500;
  }

  button.text {
    cursor: default;
  }

  button.text:hover {
    color: var(--color-text);
    text-decoration: underline;
    text-decoration-color: var(--color-border-strong);
    text-underline-offset: 3px;
  }

  .code {
    font-family: var(--font-mono);
    font-size: var(--text-caption);
  }

  .detail {
    overflow: hidden;
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 16px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
