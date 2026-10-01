<script lang="ts">
  import type { WorkSession } from "$domain/work";
  import type { WorkHumanSession } from "$domain/work-human";
  import AskCard from "./AskCard.svelte";
  import { asksFor, asksOf, openAsks, type Ask } from "./asks";
  import { runActions } from "./actions";

  /**
   * The questions a run is waiting on, bound to its operations. The island
   * holds the open ones; a part's row holds its own, open and decided.
   */
  let {
    session,
    human = null,
    part = null,
    placement = "island",
    seed = 0,
    onopenpage,
  }: {
    session: WorkSession;
    /** The held pages of the work, for sign-in walls. */
    human?: WorkHumanSession | null;
    /** Only this part's asks, open and decided, as its row shows them. */
    part?: string | null;
    placement?: "canvas" | "island";
    seed?: number;
    /** Presents the page a step works on: to sign in, or to see what will be sent. */
    onopenpage?: (step: string) => void;
  } = $props();

  const work = $derived(session.projection?.work.id ?? null);
  const execution = $derived(session.projection?.executions.at(-1) ?? null);
  const held = $derived(work && human ? (human.pages.get(work) ?? []) : []);
  const asks = $derived<Ask[]>(
    execution
      ? part
        ? asksFor(asksOf(execution, session.pages, held), part)
        : openAsks(asksOf(execution, session.pages, held))
      : [],
  );
  const actions = $derived(
    execution
      ? runActions(
          session,
          execution.id,
          human && work ? { session: human, work } : undefined,
          onopenpage,
        )
      : null,
  );
</script>

{#if actions && asks.length}
  <div class="asks {placement}">
    {#each asks as ask (ask.step)}<AskCard {ask} {actions} {placement} {seed} />{/each}
  </div>
{/if}

<style>
  .asks {
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
  }

  .canvas {
    gap: 10px;
  }

  /* One question at a time reads calmest; later ones wait under a hairline. */
  .island > :global(* + *) {
    border-block-start: 1px solid var(--color-border);
  }
</style>
