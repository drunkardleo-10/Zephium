<script lang="ts">
  import { tokenize } from "$shared/ui/data/Code/tokenize";
  import * as m from "$shared/i18n/messages";
  import type { DiffView, ObjectActions } from "../../lib/board/types";
  import { splitPath } from "./path";
  /** A change to one file, read like a good review: path, summary, hunks with their gutters. */
  let {
    object,
    actions = {},
    centre = false,
  }: {
    object: DiffView;
    actions?: ObjectActions;
    /** Opened in the centre: all of it, at reading size. */
    centre?: boolean;
  } = $props();
  /** Lines the canvas shows before the rest are read in the centre. */
  const LINES = 24;
  const path = $derived(splitPath(object.path));
  const added = $derived(
    object.hunks.reduce(
      (sum, hunk) => sum + hunk.lines.filter((line) => line.op === "add").length,
      0,
    ),
  );
  const removed = $derived(
    object.hunks.reduce(
      (sum, hunk) => sum + hunk.lines.filter((line) => line.op === "del").length,
      0,
    ),
  );
  /** Five blocks, shared between additions and removals as the review tools draw them. */
  const blocks = $derived.by(() => {
    const total = added + removed;
    if (!total) return [];
    const plus = Math.round((added / total) * 5);
    return Array.from({ length: 5 }, (_, index) => (index < plus ? "plus" : "minus"));
  });
  type Row =
    | { kind: "hunk"; at: number }
    | {
        kind: "line";
        op: "ctx" | "add" | "del";
        old: number | null;
        new: number | null;
        tokens: ReturnType<typeof tokenize>[number];
      };
  const rows = $derived.by(() => {
    const out: Row[] = [];
    object.hunks.forEach((hunk, index) => {
      if (index > 0 || hunk.oldStart > 1) out.push({ kind: "hunk", at: hunk.newStart });
      const tokens = tokenize(
        object.language,
        hunk.lines.map((line) => line.text).join("\n") + "\n",
      );
      let old = hunk.oldStart;
      let next = hunk.newStart;
      hunk.lines.forEach((line, at) => {
        out.push({
          kind: "line",
          op: line.op,
          old: line.op === "add" ? null : old++,
          new: line.op === "del" ? null : next++,
          tokens: tokens[at] ?? [{ kind: "plain", text: line.text }],
        });
      });
    });
    return out;
  });
  const shown = $derived(centre ? rows : rows.slice(0, LINES));
  const digits = $derived(
    String(Math.max(...object.hunks.map((hunk) => hunk.newStart + hunk.lines.length), 1)).length,
  );
  const hidden = $derived(
    rows.filter((row) => row.kind === "line").length -
      shown.filter((row) => row.kind === "line").length,
  );
</script>

<section class="diff" aria-label={object.path}>
  <header>
    <p class="path">
      <span class="folder">{path.folder}</span><span class="name">{path.name}</span>
    </p>
    <p class="stat">
      <span class="added">+{added}</span><span class="removed">−{removed}</span>
      <span class="blocks" aria-hidden="true"
        >{#each blocks as block, index (index)}<span class={block}></span>{/each}</span
      >
    </p>
  </header>
  {#if object.summary}<p class="summary">{object.summary}</p>{/if}
  <div class="hunks" style:--digits={digits}>
    {#each shown as row, index (index)}
      {#if row.kind === "hunk"}<div class="fold">
          {m.work_diff_line({ line: row.at })}
        </div>{:else}<div class="row {row.op}">
          <span class="n">{row.old ?? ""}</span><span class="n">{row.new ?? ""}</span><span
            class="sign"
            aria-label={row.op === "add"
              ? m.work_diff_added()
              : row.op === "del"
                ? m.work_diff_removed()
                : undefined}>{row.op === "add" ? "+" : row.op === "del" ? "−" : ""}</span
          ><code
            >{#each row.tokens as token, at (at)}{#if token.kind === "plain"}{token.text}{:else}<span
                  class={token.kind}>{token.text}</span
                >{/if}{/each}</code
          >
        </div>{/if}
    {/each}
  </div>
  {#if hidden > 0}<button
      type="button"
      class="all nodrag nopan"
      onclick={() => actions.open?.(object.id)}
      >{m.work_object_show_all_lines({ count: hidden + LINES })}</button
    >{/if}
</section>

<style>
  .diff {
    display: flex;
    flex-direction: column;
    gap: 10px;
    box-sizing: border-box;
    inline-size: 100%;
    padding: 16px 0 12px;
    overflow: hidden;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
    color: var(--color-text);
  }

  header,
  .summary,
  .all {
    margin-inline: 18px;
  }

  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 16px;
  }

  .path {
    min-inline-size: 0;
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-label);
    overflow-wrap: anywhere;
  }

  .folder {
    color: var(--color-muted);
  }

  .name {
    font-weight: 600;
  }

  .stat {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-label);
    font-weight: 600;
  }

  .added {
    color: var(--color-success);
  }

  .removed {
    color: var(--color-danger);
  }

  .blocks {
    display: inline-flex;
    gap: 2px;
    margin-inline-start: 2px;
  }

  .blocks span {
    inline-size: 7px;
    block-size: 7px;
    border-radius: var(--radius-capsule);
  }

  .blocks .plus {
    background: var(--color-success);
  }

  .blocks .minus {
    background: var(--color-danger);
  }

  .summary {
    margin-block: 0 4px;
    color: var(--color-label-secondary);
    font-size: var(--text-body);
    line-height: 19px;
  }

  .hunks {
    --line: 20px;

    display: flex;
    flex-direction: column;
    border-block: 1px solid var(--color-border);
    font-family: var(--font-mono);
    font-size: var(--text-label);
  }

  .fold {
    padding: 4px 18px;
    background: var(--color-fill);
    color: var(--color-muted);
    font-family: var(--font-sans);
    font-size: var(--text-caption);
    line-height: 18px;
  }

  .row {
    display: grid;
    grid-template-columns:
      calc(var(--digits) * 1ch + 20px) calc(var(--digits) * 1ch + 12px)
      16px minmax(0, 1fr);
    min-block-size: var(--line);
    line-height: var(--line);
  }

  .n {
    padding-inline-end: 8px;
    color: var(--color-faint);
    font-variant-numeric: tabular-nums;
    text-align: end;
    user-select: none;
  }

  .sign {
    text-align: center;
    user-select: none;
  }

  code {
    padding-inline-end: 18px;
    font: inherit;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .row.add {
    background: color-mix(in oklab, var(--color-success) 14%, transparent);
  }

  .row.add .sign,
  .row.add .n {
    color: var(--color-success);
  }

  .row.del {
    background: color-mix(in oklab, var(--color-danger) 13%, transparent);
  }

  .row.del .sign,
  .row.del .n {
    color: var(--color-danger);
  }

  .comment {
    color: var(--color-code-comment);
    font-style: italic;
  }

  .keyword,
  .tag {
    color: var(--color-code-keyword);
  }

  .string {
    color: var(--color-code-string);
  }

  .number {
    color: var(--color-code-number);
  }

  .function {
    color: var(--color-code-function);
  }

  .type {
    color: var(--color-code-type);
  }

  .property {
    color: var(--color-code-property);
  }

  .attribute {
    color: var(--color-code-attribute);
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
</style>
