<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Tone } from "../types";
  let {
    children,
    tone = "neutral",
    dot = false,
  }: { children: Snippet; tone?: Tone; dot?: boolean } = $props();
</script>

<span class="ui-badge" data-tone={tone}
  >{#if dot}<span class="dot" aria-hidden="true"></span>{/if}{@render children()}</span
>

<style>
  /* Status is text. Color appears only as a 5px dot, and only for a reason. */
  .ui-badge {
    --tone: var(--color-faint);

    display: inline-flex;
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    padding: 2px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    box-shadow: inset 0 0 0 1px var(--color-border);
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
  }

  .ui-badge[data-tone="accent"] {
    --tone: var(--color-accent);
  }

  .ui-badge[data-tone="success"] {
    --tone: var(--color-success);
  }

  .ui-badge[data-tone="warning"] {
    --tone: var(--color-warning);
  }

  .ui-badge[data-tone="danger"] {
    --tone: var(--color-danger);

    color: var(--color-danger);
  }

  .dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--tone);
  }

  @media (forced-colors: active) {
    .ui-badge {
      border: 1px solid ButtonText;
    }

    .dot {
      background: CanvasText;
    }
  }
</style>
