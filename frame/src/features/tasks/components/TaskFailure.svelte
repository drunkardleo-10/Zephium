<script lang="ts">
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";
  import type { TaskSession } from "$domain/resources";

  let { session }: { session: TaskSession } = $props();
  let message = $derived(
    session.failure === "conflict"
      ? m.task_save_conflict()
      : session.failure === "outcome_unknown"
        ? m.task_save_unknown()
        : session.failure === "invalid"
          ? m.task_invalid_title()
          : m.task_save_failed(),
  );
</script>

{#if session.failure}<div class="task-error" role="alert">
    <p>{message}</p>
    <div class="task-error-actions">
      <Button size="compact" onclick={() => void session.retry()}
        >{session.failure === "conflict" ? m.task_save_local() : m.surface_retry()}</Button
      ><Button size="compact" variant="ghost" onclick={() => void session.discard()}
        >{m.task_discard()}</Button
      >
    </div>
  </div>{/if}

<style>
  .task-error {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: 10px;
    margin-block: 4px 10px;
    padding: 12px 14px;
    border-radius: var(--radius-row);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-danger) 35%, transparent);
  }

  p {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-label);
    line-height: 1.5;
  }

  .task-error-actions {
    display: flex;
    gap: 6px;
  }
</style>
