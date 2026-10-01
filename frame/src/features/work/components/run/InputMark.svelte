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
  import * as m from "$shared/i18n/messages";

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
  /** What kind of thing it is, under its name: "Skill", "Folder", "3 tabs". */
  const caption = $derived.by(() => {
    if (input.folder) return m.work_input_folder();
    const count = input.count ?? 0;
    switch (input.kind) {
      case "memory":
        return m.work_input_memory();
      case "skill":
        return m.work_input_skill();
      case "history":
        return m.work_input_history();
      case "notes":
        return count > 1 ? m.work_input_notes({ count }) : m.work_input_note();
      case "tabs":
        return count > 1 ? m.work_input_tabs({ count }) : m.work_input_tab();
      case "files":
        return count > 1 ? m.work_input_files({ count }) : m.work_input_file();
      case "connection":
        return m.work_input_connection();
      case "work":
        return m.work_input_work();
    }
  });
</script>

<!--
  What the run drew on, set as the mirror of a part's row: its name and what
  kind of thing it is read toward the request, its mark where the line leaves.
-->
<div class="input" class:lit={!!input.lit} title={input.label}>
  <span class="words">
    <span class="label">{input.label}</span>
    <span class="caption">{caption}</span>
  </span>
  <span class="tile" class:folder={!!input.folder}
    ><Icon icon={input.folder ? Folder01Icon : GLYPH[input.kind]} size={15} /></span
  >
</div>

<style>
  .input {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 100%;
    color: var(--color-muted);
  }

  .words {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    min-inline-size: 0;
    text-align: end;
  }

  .label {
    max-inline-size: 100%;
    overflow: hidden;
    color: var(--color-text);
    font-size: var(--text-body);
    font-weight: 500;
    letter-spacing: -0.003em;
    line-height: 17px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .caption {
    color: var(--color-faint);
    font-size: var(--text-caption);
    line-height: 14px;
  }

  .tile {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 30px;
    block-size: 30px;
    border-radius: var(--radius-inset);
    background: var(--color-surface);
    box-shadow:
      0 0 0 1px var(--color-border),
      var(--shadow-raised);
    color: var(--color-muted);
    transition: color var(--motion-base) var(--ease-out);
  }

  .lit .tile {
    color: var(--color-text);
  }

  .lit .tile.folder {
    color: var(--color-accent);
  }
</style>
