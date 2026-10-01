<script lang="ts">
  import { untrack } from "svelte";
  import * as m from "$shared/i18n/messages";
  import { WorkMemorySession } from "$domain/work-context";
  import Icon from "$shared/ui/Icon";
  import { Brain02Icon } from "./icons";

  /**
   * What a run remembered about the person, set on the canvas beside it as
   * one quiet line each, with Undo. Rust keeps the facts; this only reads the
   * work's and forgets one when asked.
   */
  let {
    profile,
    work,
    execution = null,
    version = 0,
  }: {
    profile: string;
    work: string;
    /** Only this run's facts. */
    execution?: string | null;
    /** Changes when the run moves on, so a new fact shows without a reload. */
    version?: number;
  } = $props();

  let session = $state.raw<WorkMemorySession | null>(null);
  $effect(() => {
    const owner = new WorkMemorySession(profile, { work });
    session = owner;
    void owner.start();
    return () => owner.dispose();
  });
  $effect(() => {
    void version;
    untrack(() => {
      if (session?.loaded) void session.refresh();
    });
  });
  const facts = $derived(
    (session?.memories ?? []).filter((fact) => !execution || fact.execution === execution),
  );
</script>

{#if facts.length}
  <ul class="remembered" aria-label={m.work_remembered_label()}>
    {#each facts as fact (fact.id)}
      <li>
        <span class="glyph" aria-hidden="true"><Icon icon={Brain02Icon} size={13} /></span>
        <span class="words"
          ><span class="lead">{m.work_remembered()}</span><span class="fact">{fact.text}</span
          ></span
        >
        <button
          type="button"
          class="undo"
          disabled={session?.busy}
          onclick={() => void session?.change({ kind: "forget", id: fact.id })}
          >{m.work_remembered_undo()}</button
        >
      </li>
    {/each}
  </ul>
{/if}

<style>
  .remembered {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-inline-size: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
    animation: remembered-in var(--motion-slow) var(--ease-emphasized);
  }

  .glyph {
    display: grid;
    flex: none;
    place-items: center;
    align-self: center;
    inline-size: 16px;
    block-size: 16px;
    color: var(--color-faint);
  }

  .words {
    display: flex;
    gap: 6px;
    min-inline-size: 0;
  }

  .lead {
    flex: none;
    color: var(--color-faint);
  }

  .fact {
    min-inline-size: 0;
    overflow: hidden;
    color: var(--color-text);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .undo {
    flex: none;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    text-decoration: underline;
    text-decoration-color: var(--color-border-strong);
    text-underline-offset: 3px;
    cursor: default;
  }

  .undo:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .undo:hover:not(:disabled) {
    color: var(--color-text);
    text-decoration-color: currentcolor;
  }

  @keyframes remembered-in {
    from {
      opacity: 0;
      translate: 0 3px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    li {
      animation: none;
    }
  }
</style>
