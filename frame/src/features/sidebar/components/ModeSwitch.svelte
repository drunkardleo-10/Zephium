<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { surface } from "$domain/surface";
  import { commands } from "$shared/ipc/bindings";
  let { compact = false }: { compact?: boolean } = $props();
</script>

{#if compact}<button
    type="button"
    class="compact-mode-word"
    aria-haspopup="menu"
    title={m.ui_mode_work_hint()}
    onclick={() => void commands.runCommand("mode.choose")}
    >{surface.currentPage() === "work" ? m.mode_work() : m.mode_browse()}</button
  >
{:else}
  <div class="mode-footer">
    <div class="mode-options" role="group" aria-label={m.mode_switch()}>
      {#each ["browse", "work"] as mode (mode)}
        <button
          type="button"
          aria-pressed={(surface.currentPage() === "work") === (mode === "work")}
          onclick={() => void surface.open(mode === "work" ? "work" : null)}
          >{mode === "work" ? m.mode_work() : m.mode_browse()}</button
        >
      {/each}
    </div>
  </div>
{/if}

<style>
  .mode-options {
    display: flex;
    padding: 3px;
    border-radius: var(--radius-control);
    background: var(--color-fill);
  }

  .mode-options button {
    flex: 1;
    border: 0;
    border-radius: var(--radius-control);
    padding: 8px;
    font: inherit;
    font-size: var(--text-caption);
    background: transparent;
    color: var(--color-muted);
    cursor: default;
  }

  .mode-options button[aria-pressed="true"] {
    background: var(--color-control);
    color: var(--color-text);
    box-shadow: var(--shadow-control);
  }

  .mode-options button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>
