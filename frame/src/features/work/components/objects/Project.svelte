<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";
  import type { ObjectActions, ProjectView } from "../../lib/board/types";
  import { File01Icon, Folder01Icon, GitBranchIcon } from "./icons";
  import Mark from "./Mark.svelte";
  import { treeRows } from "./project";
  /** A folder read as a project: what it is, what it's built with, its shape, its commands, its state. */
  let {
    object,
    actions = {},
    centre = false,
  }: {
    object: ProjectView;
    actions?: ObjectActions;
    /** Opened in the centre: the whole tree. */
    centre?: boolean;
  } = $props();
  const ROWS = 18;
  const rows = $derived(treeRows(object.tree, object.more));
  const shown = $derived(centre ? rows : rows.slice(0, ROWS));
  const git = $derived(object.git);
  /** The folder as a person reads it: their home as `~`. */
  const where = $derived(object.root.replace(/^\/(?:Users|home)\/[^/]+/u, "~"));
</script>

<section class="project" aria-label={object.name}>
  <header>
    <span class="glyph" aria-hidden="true"><Icon icon={Folder01Icon} size={22} /></span>
    <div class="head">
      <h3>{object.name}</h3>
      {#if object.summary}<p class="summary">{object.summary}</p>{/if}
      {#if object.root || git}<p class="meta">
          {#if object.root}<span class="path" title={object.root}>{where}</span>{/if}
          {#if git}<span class="git">
              <Icon icon={GitBranchIcon} size={13} />
              {#if git.branch}<span class="branch">{git.branch}</span>{/if}
              <span class="changes"
                >{git.changed
                  ? m.work_project_changed({ count: git.changed })
                  : m.work_project_clean()}</span
              >
              {#if git.ahead}<span class="sync">{m.work_project_ahead({ count: git.ahead })}</span
                >{/if}
              {#if git.behind}<span class="sync"
                  >{m.work_project_behind({ count: git.behind })}</span
                >{/if}
            </span>{/if}
        </p>{/if}
    </div>
  </header>

  {#if object.stack.length}<ul class="stack" aria-label={m.work_project_stack()}>
      {#each object.stack as item (item.name)}<li title={item.role}>
          {#if item.host}<Mark address={item.host} size={16} />{/if}
          <span class="name">{item.name}</span>
          {#if item.version}<span class="version">{item.version}</span>{/if}
        </li>{/each}
    </ul>{/if}

  <div class="body" class:single={!object.scripts.length || !rows.length}>
    {#if rows.length}<div class="column">
        <h4>{m.work_project_structure()}</h4>
        <ul class="tree">
          {#each shown as row, index (index)}<li
              class:folder={"folder" in row && row.folder}
              style:padding-inline-start={`${row.depth * 18}px`}
            >
              {#if "more" in row}<span class="count rest"
                  >{m.work_project_more({ count: row.more })}</span
                >{:else}<Icon icon={row.folder ? Folder01Icon : File01Icon} size={14} /><span
                  class="entry">{row.name}</span
                >{#if row.holds}<span class="count">{row.holds}</span>{/if}{/if}
            </li>{/each}
        </ul>
        {#if shown.length < rows.length}<button
            type="button"
            class="all nodrag nopan"
            onclick={() => actions.open?.(object.id)}>{m.work_project_show_tree()}</button
          >{/if}
      </div>{/if}
    {#if object.scripts.length}<div class="column">
        <h4>{m.work_project_scripts()}</h4>
        <dl class="scripts">
          {#each object.scripts as script, index (index)}<div>
              <dt>
                <span>{script.name}</span>{#if script.source}<span class="source"
                    >{script.source}</span
                  >{/if}
              </dt>
              {#if script.command && script.command !== script.name}<dd>{script.command}</dd>{/if}
            </div>{/each}
        </dl>
      </div>{/if}
  </div>
</section>

<style>
  .project {
    display: flex;
    flex-direction: column;
    gap: 18px;
    box-sizing: border-box;
    inline-size: 100%;
    padding: 20px 22px 22px;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
    color: var(--color-text);
    container-type: inline-size;
  }

  header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }

  .glyph {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 40px;
    block-size: 40px;
    border-radius: var(--radius-row);
    background: var(--color-fill);
    color: var(--color-muted);
  }

  .head {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-inline-size: 0;
    padding-block-start: 1px;
  }

  h3 {
    margin: 0;
    font-size: var(--text-headline);
    font-weight: 650;
    line-height: 1.1;
    letter-spacing: -0.02em;
    overflow-wrap: anywhere;
  }

  .summary {
    margin: 2px 0 0;
    max-inline-size: 64ch;
    color: var(--color-label-secondary);
    font-size: var(--text-reading);
    line-height: 1.4;
    text-wrap: pretty;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 14px;
    margin: 4px 0 0;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .path {
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }

  .git {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .branch {
    color: var(--color-label-secondary);
    font-weight: 500;
  }

  .changes::before,
  .sync::before {
    margin-inline-end: 6px;
    content: "·";
  }

  .stack {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .stack li {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    block-size: 30px;
    padding: 0 12px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    font-size: var(--text-body);
  }

  .stack .name {
    font-weight: 550;
  }

  .version {
    color: var(--color-muted);
    font-variant-numeric: tabular-nums;
  }

  .body {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 24px 32px;
    padding-block-start: 16px;
    border-block-start: 1px solid var(--color-border);
  }

  .body.single {
    grid-template-columns: minmax(0, 1fr);
  }

  @container (inline-size < 520px) {
    .body {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .column {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-inline-size: 0;
  }

  h4 {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 600;
    letter-spacing: 0.06em;
    line-height: 16px;
    text-transform: uppercase;
  }

  .tree {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--text-body);
  }

  .tree li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-block-size: 24px;
    color: var(--color-faint);
  }

  .entry {
    min-inline-size: 0;
    color: var(--color-label-secondary);
    overflow-wrap: anywhere;
  }

  .folder .entry {
    color: var(--color-text);
    font-weight: 500;
  }

  .count {
    color: var(--color-faint);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
  }

  .count.rest {
    padding-inline-start: 22px;
  }

  .all {
    align-self: flex-start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
  }

  .all:hover {
    color: var(--color-text);
  }

  .scripts {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0;
  }

  .scripts div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  dt {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    font-family: var(--font-mono);
    font-size: var(--text-body);
    font-weight: 550;
    overflow-wrap: anywhere;
  }

  .source {
    flex: none;
    color: var(--color-faint);
    font-family: var(--font-sans);
    font-size: var(--text-caption);
    font-weight: 400;
  }

  dd {
    margin: 0;
    color: var(--color-muted);
    font-family: var(--font-mono);
    font-size: var(--text-label);
    line-height: 17px;
    overflow-wrap: anywhere;
  }
</style>
