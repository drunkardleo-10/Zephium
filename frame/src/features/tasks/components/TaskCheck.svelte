<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import { Tick02Icon } from "@hugeicons/core-free-icons";
  import type { TaskStatus } from "$domain/resources";

  let {
    status,
    label,
    size = "regular",
    disabled = false,
    tabindex = -1,
    ontoggle,
  }: {
    status: TaskStatus;
    size?: "regular" | "small";
    /** Already carries the state, so a reader hears what the box means. */
    label: string;
    disabled?: boolean;
    tabindex?: number;
    ontoggle: (next: TaskStatus) => void;
  } = $props();
</script>

<!--
  The single most-pressed control in the feature, so it is a real control: the
  checkbox recipe from the kit, not a glyph in a button. A task an agent holds
  keeps the same box — the reader can always take it back by ticking it.
-->
<button
  type="button"
  class="task-check"
  role="checkbox"
  aria-checked={status === "done"}
  aria-label={label}
  title={label}
  data-status={status}
  data-size={size}
  {disabled}
  {tabindex}
  onclick={() => ontoggle(status === "done" ? "open" : "done")}
>
  {#if status === "done"}<span class="mark" aria-hidden="true"
      ><Icon icon={Tick02Icon} size={size === "small" ? 10 : 12} strokeWidth={2.6} /></span
    >{:else if status !== "open"}<span class="state" aria-hidden="true"></span>{/if}
</button>

<style>
  .task-check {
    display: grid;
    place-items: center;
    flex: none;
    box-sizing: border-box;
    position: relative;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 1.5px solid var(--color-faint);
    border-radius: var(--radius-capsule);
    background: transparent;
    color: var(--color-on-lit);
    box-shadow: var(--shadow-control);
    cursor: default;
    transition:
      background-color var(--motion-instant) var(--ease-smooth),
      box-shadow var(--motion-instant) var(--ease-smooth),
      scale var(--motion-slow) var(--ease-smooth);
  }

  .task-check::before {
    position: absolute;
    inset: -5px;
    content: "";
  }

  .task-check[data-size="small"] {
    width: 15px;
    height: 15px;
    border-width: 1.25px;
  }

  .task-check:disabled {
    opacity: 0.5;
  }

  .task-check:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 3px;
  }

  .task-check:active:not(:disabled) {
    scale: 0.92;
    transition-duration: var(--motion-fast);
  }

  .task-check:not([data-status="done"]):hover:not(:disabled) {
    background: var(--color-control-hover);
  }

  .task-check[data-status="done"] {
    background: var(--color-lit);
    border-color: var(--color-lit);
  }

  /* A finished box stays filled under the pointer; it only brightens. */
  .task-check[data-status="done"]:hover:not(:disabled) {
    background: var(--color-lit-hover);
    border-color: var(--color-lit-hover);
  }

  /* Something is working on it: a filled centre, held still. A list that
     animates forever is a list nobody can read. */
  .task-check[data-status="active"] .state {
    width: 7px;
    height: 7px;
    border-radius: 2px;
    background: var(--color-on-control-strong);
  }

  /* The one place a task list earns colour: it stopped and needs a person. */
  .task-check[data-status="blocked"] {
    border-color: var(--color-warning);
  }

  .task-check[data-status="blocked"] .state {
    width: 7px;
    height: 7px;
    border-radius: var(--radius-capsule);
    background: var(--color-warning);
  }

  .mark {
    display: grid;
    place-items: center;
    animation: task-mark var(--motion-slow) var(--ease-spring);
  }

  @keyframes task-mark {
    from {
      opacity: 0;
      transform: scale(0.4);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .mark {
      animation: none;
    }
  }

  @media (forced-colors: active) {
    .task-check {
      border: 1px solid ButtonText;
    }

    .task-check[data-status="done"] {
      background: Highlight;
      color: HighlightText;
    }
  }
</style>
