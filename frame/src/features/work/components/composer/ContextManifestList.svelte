<script lang="ts">
  import type { WorkContextDisclosureV1, WorkContextItemKind } from "$shared/ipc/bindings";
  import { formatBytes } from "../../lib/context-selection";
  import * as m from "$shared/i18n/messages";
  let { disclosure, compact = false }: { disclosure: WorkContextDisclosureV1; compact?: boolean } =
    $props();
  const kinds: Record<WorkContextItemKind, () => string> = {
    note: m.work_context_kind_note,
    task: m.work_context_kind_task,
    object: m.work_context_kind_object,
    tab: m.work_context_kind_tab,
    objective: m.work_context_kind_objective,
    artifact: m.work_context_kind_artifact,
    subject: m.work_context_kind_subject,
    finding: m.work_context_kind_finding,
    decision: m.work_context_kind_decision,
  };
</script>

<ul class="manifest" class:compact>
  {#each disclosure.items as item (`${item.element}:${item.implicit ? "d" : "s"}`)}
    <li>
      <span class="kind">{kinds[item.kind]()}</span>
      <span class="title">{item.title || m.work_env_untitled_tab()}</span>
      <span class="meta">
        <span class="visibility" class:private={item.visibility === "private"}
          >{item.visibility === "private"
            ? m.work_context_private()
            : m.work_context_public()}</span
        >
        {formatBytes(item.bytes)}{#if item.truncated}<span class="truncated"
            >{m.work_context_truncated()}</span
          >{/if}
      </span>
    </li>
  {/each}
</ul>
<p class="total">{m.work_context_total({ size: formatBytes(disclosure.total_bytes) })}</p>

<style>
  .manifest {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: baseline;
    gap: 8px;
    font-size: var(--text-caption);
  }

  .kind {
    padding: 1px 6px;
    border-radius: var(--radius-xs);
    background: var(--color-fill);
    color: var(--color-muted);
    font-size: 11px;
    text-transform: capitalize;
  }

  .title {
    overflow: hidden;
    color: var(--color-text);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    display: inline-flex;
    gap: 6px;
    color: var(--color-faint);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .visibility.private {
    color: var(--color-warning);
  }

  .truncated {
    color: var(--color-faint);
  }

  .total {
    margin: 6px 0 0;
    color: var(--color-faint);
    font-size: 11px;
  }

  .compact li {
    font-size: 11.5px;
  }
</style>
