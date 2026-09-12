<script lang="ts">
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";
  let {
    state: status,
    onretry,
    ondiscard,
    onkeep,
  }: {
    state: "saved" | "unsaved" | "saving" | "conflict" | "unknown" | "failed";
    onretry: () => void;
    ondiscard: () => void;
    onkeep: () => void;
  } = $props();
  let confirm = $state(false);
  let slowSave = $state(false);
  $effect(() => {
    slowSave = false;
    if (status !== "saving") return;
    const timer = setTimeout(() => {
      slowSave = true;
    }, 800);
    return () => clearTimeout(timer);
  });
  $effect(() => {
    if (status !== "conflict") confirm = false;
  });
  let label = $derived(
    status === "saved"
      ? m.resource_saved()
      : status === "unsaved"
        ? m.resource_unsaved()
        : status === "saving"
          ? m.resource_saving()
          : status === "conflict"
            ? m.resource_conflict()
            : status === "unknown"
              ? m.resource_unknown()
              : m.resource_save_failed(),
  );
</script>

<div class="save-status" class:problem={["conflict", "unknown", "failed"].includes(status)}>
  <p
    role="status"
    aria-live="polite"
    class:quiet={status === "unsaved" || (status === "saving" && !slowSave)}
  >
    {status === "unsaved" || (status === "saving" && !slowSave) ? "" : label}
  </p>
  {#if status === "unknown" || status === "failed"}<Button size="compact" onclick={onretry}
      >{status === "unknown" ? m.resource_resolve() : m.surface_retry()}</Button
    >{/if}
  {#if status === "conflict"}<div class="actions">
      <Button size="compact" onclick={ondiscard}>{m.resource_load_saved()}</Button><Button
        size="compact"
        onclick={() => (confirm = !confirm)}>{m.resource_keep_draft()}</Button
      >
    </div>
    {#if confirm}<p>{m.resource_overwrite_explanation()}</p>
      <Button
        size="compact"
        onclick={() => {
          confirm = false;
          onkeep();
        }}>{m.resource_confirm_replace()}</Button
      >{/if}{/if}
</div>

<style>
  .save-status {
    padding: 4px 16px;
    min-height: 20px;
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  p {
    margin: 0;
    line-height: 20px;
    min-height: 20px;
  }

  .quiet {
    visibility: hidden;
  }

  .problem {
    padding-block: 10px;
    background: var(--color-fill);
    border-block-end: 1px solid var(--color-border);
  }

  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-block-start: 8px;
  }
</style>
