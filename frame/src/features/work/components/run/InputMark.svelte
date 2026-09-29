<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import type { CanvasItem } from "../../lib/canvas-model";
  import {
    AiBrain01Icon,
    BookOpen01Icon,
    BrowserIcon,
    Files01Icon,
    Folder01Icon,
    Note01Icon,
    Plug01Icon,
    WorkHistoryIcon,
    LayoutGridIcon,
  } from "../../lib/icons";

  let { item }: { item: CanvasItem } = $props();
  const input = $derived(item.input!);
  const GLYPH = {
    memory: AiBrain01Icon,
    skill: BookOpen01Icon,
    history: WorkHistoryIcon,
    notes: Note01Icon,
    tabs: BrowserIcon,
    files: Files01Icon,
    connection: Plug01Icon,
    work: LayoutGridIcon,
  } as const;
</script>

<!-- What the agent drew on: a small mark and its words, lit once read. -->
<div class="input" class:lit={!!input.lit} title={input.label}>
  <span class="glyph"
    ><Icon icon={input.folder ? Folder01Icon : GLYPH[input.kind]} size={14} /></span
  >
  <span class="label">{input.label}</span>
</div>

<style>
  .input {
    display: flex;
    align-items: center;
    gap: 8px;
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    padding: 0 10px 0 4px;
    color: var(--color-muted);
    font-size: var(--text-label);
    font-weight: 500;
    transition: color var(--motion-base) var(--ease-out);
  }

  .glyph {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 22px;
    block-size: 22px;
    border-radius: var(--radius-inset);
    background: var(--color-fill);
    transition:
      background-color var(--motion-base) var(--ease-out),
      color var(--motion-base) var(--ease-out);
  }

  .lit {
    color: var(--color-text);
  }

  .lit .glyph {
    background: var(--color-accent-soft);
    color: var(--color-accent);
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
