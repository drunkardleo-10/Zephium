<script lang="ts">
  import AskCard from "../components/asks/AskCard.svelte";
  import type { Ask } from "../components/asks/asks";
  import type { AskActions } from "../components/asks/actions";
  /** Each ask as it stands on the canvas and as the island holds it. */
  let { rows, actions }: { rows: readonly { name: string; ask: Ask }[]; actions: AskActions } =
    $props();
</script>

<div class="sheet">
  {#each rows as row (row.name)}
    <div class="row" data-name={row.name}>
      <div class="canvas"><AskCard ask={row.ask} {actions} seed={3} /></div>
      {#if row.ask.state === "open"}<div class="island">
          <AskCard ask={row.ask} {actions} placement="island" seed={3} />
        </div>{/if}
    </div>
  {/each}
</div>

<style>
  .sheet {
    display: flex;
    flex-direction: column;
    inline-size: max-content;
    background: var(--color-canvas);
    color: var(--color-text);
    font-family: var(--font-sans);
  }

  .row {
    display: flex;
    align-items: flex-start;
    gap: 48px;
    padding: 32px;
    background: var(--color-canvas);
  }

  .canvas {
    inline-size: 360px;
  }

  /* The island's own material, as AgentLine draws it. */
  .island {
    inline-size: 440px;
    border-radius: var(--radius-panel);
    background: var(--color-menu);
    box-shadow: var(--shadow-menu);
  }
</style>
