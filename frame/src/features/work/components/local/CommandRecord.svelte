<script lang="ts">
  import type { WorkCommandRecordV1 } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";
  import { commandSummary } from "../../lib/local-steps";
  import { homePath } from "../../lib/work-files";
  let { record }: { record: WorkCommandRecordV1 } = $props();
  let copyState = $state<"idle" | "copied" | "failed">("idle");
  async function copy() {
    try {
      await navigator.clipboard.writeText(record.command.text);
      copyState = "copied";
    } catch {
      copyState = "failed";
    }
  }
</script>

<section aria-label={m.work_local_record()}>
  <p class="path">{homePath(record.command.cwd)}</p>
  <pre class="command">{record.command.command}</pre>
  <div class="facts">
    <span>{commandSummary(record)}</span>
    <Button size="compact" onclick={() => void copy()}>{m.work_local_copy_output()}</Button>
  </div>
  {#if record.command.truncated}<p>
      {m.work_local_output_truncated({ bytes: String(record.command.bytes) })}
    </p>{/if}
  <textarea
    class="output"
    readonly
    rows="12"
    aria-label={m.work_local_output()}
    value={record.command.text || m.work_local_no_output()}></textarea>
  <span class="status" role="status"
    >{copyState === "copied"
      ? m.work_local_copied()
      : copyState === "failed"
        ? m.work_local_copy_failed()
        : ""}</span
  >
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-block-size: 0;
  }

  p,
  .facts,
  .status {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .facts {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .path {
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }

  pre,
  .output {
    margin: 0;
    padding: 12px;
    border-radius: var(--radius-row);
    background: var(--color-fill);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .output {
    min-block-size: 120px;
    max-block-size: 360px;
    border: none;
    resize: vertical;
    overflow: auto;
  }

  .output:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>
