<script lang="ts">
  import type { WorkExecutionFact } from "$shared/ipc/bindings";
  import HostGlyph from "../cards/HostGlyph.svelte";
  import LiftHeader from "../LiftHeader.svelte";
  import { sourceRows, type SourceRow } from "../../lib/project-environment-stage";
  import { fileName } from "../../lib/work-files";
  import type { TrailLine } from "../../lib/board/trail";
  import { UserIcon } from "../../lib/icons";
  import * as m from "$shared/i18n/messages";
  let {
    request,
    trail,
    runs,
    made,
    onsource,
    onmade,
  }: {
    request: string;
    trail: readonly TrailLine[];
    runs: readonly WorkExecutionFact[];
    /** What the request's board holds, by title. */
    made: readonly { id: string; title: string }[];
    onsource: (row: SourceRow) => void;
    onmade: (id: string) => void;
  } = $props();
  const host = (url: string) => {
    try {
      return new URL(url).host.replace(/^www\./u, "");
    } catch {
      return url;
    }
  };
  /** Each step the runs took, as a person would retell it; turns without words say nothing. */
  const steps = $derived(
    runs.flatMap((run) =>
      (run.steps ?? []).flatMap((step) => {
        const kind = step.kind;
        const failed = step.status === "failed" || step.status === "cancelled";
        const line = (text: string, detail = "") => [
          { key: `${run.id}:${step.id}`, text, detail, failed, running: step.status === "running" },
        ];
        switch (kind.kind) {
          case "turn":
          case "finish":
            return step.note?.trim() ? line(step.note.trim()) : [];
          case "search":
            return line(m.work_run_searched({ query: kind.query }));
          case "discover":
            return line(m.work_run_searched({ query: kind.query }));
          case "read":
            return line(
              step.local?.page_title?.trim() || host(kind.url),
              failed ? (step.note?.trim() ?? "") : host(kind.url),
            );
          case "ask":
            return line(
              kind.prompt,
              kind.answer ? m.work_trail_you_answered({ answer: kind.answer }) : "",
            );
          case "steer":
            return line(m.work_trail_you_said({ text: kind.text }));
          case "read_file":
          case "search_files":
          case "list":
            return line(fileName(kind.path));
          case "write_file":
          case "edit_file":
            return line(m.work_run_changed({ name: fileName(kind.path) }));
          case "run_command":
            return line(kind.command);
          default:
            return [];
        }
      }),
    ),
  );
  const sources = $derived(
    runs
      .flatMap((run) => sourceRows(run))
      .filter((row, index, rows) => rows.findIndex((other) => other.key === row.key) === index),
  );
</script>

<div class="run">
  <LiftHeader kind={m.work_env_request()} title={request} icon={UserIcon} />
  <div class="sections">
    {#if trail.length}<section>
        <h3>{m.work_run_did()}</h3>
        <ul class="facts">
          {#each trail as line (line.key)}<li>
              <span>{line.text}</span>{#if line.detail}<span class="muted">{line.detail}</span>{/if}
            </li>{/each}
        </ul>
      </section>{/if}
    {#if made.length}<section>
        <h3>{m.work_run_made()}</h3>
        <ul class="made">
          {#each made as block (block.id)}<li>
              <button type="button" onclick={() => onmade(block.id)}>{block.title}</button>
            </li>{/each}
        </ul>
      </section>{/if}
    {#if steps.length}<section>
        <h3>{m.work_run_steps()}</h3>
        <ol class="steps">
          {#each steps as step (step.key)}<li
              class:failed={step.failed}
              class:running={step.running}
            >
              <span class="dot" aria-hidden="true"></span>
              <span class="words"
                ><span>{step.text}</span>{#if step.detail}<span class="muted">{step.detail}</span
                  >{/if}</span
              >
            </li>{/each}
        </ol>
      </section>{/if}
    {#if sources.length}<section>
        <h3>{m.work_sources()}</h3>
        <ul class="sources">
          {#each sources as row (row.key)}<li>
              <button type="button" onclick={() => onsource(row)}>
                <HostGlyph host={row.where} url={row.url} file={!!row.file} size={16} />
                <span class="title">{row.title}</span>
                <span class="muted">{row.where}</span>
              </button>
            </li>{/each}
        </ul>
      </section>{/if}
  </div>
</div>

<style>
  .run {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 20px 24px 24px;
  }

  .sections {
    display: flex;
    flex-direction: column;
    gap: 22px;
  }

  h3 {
    margin: 0 0 8px;
    color: var(--color-muted);
    font-size: var(--text-label);
    font-weight: 600;
  }

  ul,
  ol {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--text-body);
  }

  .facts li,
  .words {
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
  }

  .muted {
    overflow: hidden;
    color: var(--color-faint);
    font-size: var(--text-label);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .steps li {
    display: flex;
    gap: 10px;
  }

  .dot {
    flex: none;
    inline-size: 7px;
    block-size: 7px;
    margin-block-start: 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-border-strong);
  }

  .running .dot {
    background: var(--color-lit);
  }

  .failed .words {
    color: var(--color-muted);
  }

  .made button,
  .sources button {
    display: flex;
    align-items: center;
    gap: 8px;
    inline-size: 100%;
    min-inline-size: 0;
    padding: 6px 8px;
    border: 0;
    border-radius: var(--radius-row);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    text-align: start;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .made button:hover,
  .sources button:hover {
    background: var(--row-hover);
  }

  .made,
  .sources {
    gap: 0;
    margin-inline: -8px;
  }

  .title {
    min-inline-size: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
